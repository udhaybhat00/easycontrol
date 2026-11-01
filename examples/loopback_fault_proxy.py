"""Development-only, one-session loopback TCP pause fixture.

Usage: python3 -u examples/loopback_fault_proxy.py /tmp/unique-test.pause
Connect the QA client to 127.0.0.1:15999 with the real host on 5899.
Create the named file to pause forwarding; remove it to resume. Bytes remain
ordered and unchanged. No payloads or authentication credentials are logged.
The fixture is never bundled with EasyControl and exits after one session.
"""
import argparse
import asyncio
import time
from pathlib import Path


async def run(trigger: Path, maximum_seconds: int) -> None:
    forwarding = asyncio.Event()
    forwarding.set()
    active = set()
    connected_once = False
    paused_at = None

    async def handle(client_reader, client_writer):
        nonlocal connected_once
        if active:
            client_writer.close()
            await client_writer.wait_closed()
            return
        task = asyncio.current_task()
        active.add(task)
        host_writer = None
        pumps = []
        counts = {"to_host": 0, "to_client": 0}
        try:
            host_reader, host_writer = await asyncio.open_connection("127.0.0.1", 5899)
            connected_once = True
            print("CONNECTED", flush=True)

            async def pump(reader, writer, direction):
                while data := await reader.read(65536):
                    # At most one pending 64 KiB chunk per direction. TCP
                    # backpressure bounds upstream buffering during the pause.
                    await forwarding.wait()
                    writer.write(data)
                    await writer.drain()
                    counts[direction] += len(data)
                return direction + "_EOF"

            pumps = [asyncio.create_task(pump(client_reader, host_writer, "to_host")),
                     asyncio.create_task(pump(host_reader, client_writer, "to_client"))]
            done, _ = await asyncio.wait(pumps, return_when=asyncio.FIRST_COMPLETED)
            for finished in done:
                try:
                    reason = finished.result()
                except (ConnectionError, OSError) as error:
                    reason = type(error).__name__
                elapsed = None if paused_at is None else round(time.monotonic() - paused_at, 3)
                print("CLOSED", reason, "seconds_after_pause", elapsed, flush=True)
        finally:
            for pump_task in pumps:
                pump_task.cancel()
            await asyncio.gather(*pumps, return_exceptions=True)
            for writer in [client_writer, host_writer]:
                if writer is not None:
                    writer.close()
                    try:
                        await asyncio.wait_for(writer.wait_closed(), 1)
                    except (ConnectionError, OSError, asyncio.TimeoutError):
                        writer.transport.abort()
            active.discard(task)
            print("COUNTERS", counts, flush=True)

    if trigger.exists():
        raise SystemExit("Use a new, absent pause-file path")
    server = await asyncio.start_server(handle, "127.0.0.1", 15999, limit=65536)
    print("READY localhost:15999 -> localhost:5899", flush=True)
    started = time.monotonic()
    try:
        while time.monotonic() - started < maximum_seconds:
            pause = trigger.exists()
            if pause and forwarding.is_set():
                forwarding.clear()
                paused_at = time.monotonic()
                print("PAUSED", flush=True)
            elif not pause and not forwarding.is_set():
                print("RESUMED after_seconds", round(time.monotonic() - paused_at, 3), flush=True)
                forwarding.set()
                paused_at = None
            if connected_once and not active:
                break
            await asyncio.sleep(0.05)
    finally:
        # Python 3.13 Server.wait_closed also waits for active transports. Close
        # those first: waiting in `async with server` before releasing paused
        # pumps can otherwise strand the development fixture indefinitely.
        server.close()
        tasks = list(active)
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        await server.wait_closed()
    print("PROXY_STOPPED", flush=True)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("pause_file", type=Path)
    parser.add_argument("--maximum-seconds", type=int, default=300)
    args = parser.parse_args()
    if not 1 <= args.maximum_seconds <= 600:
        parser.error("maximum-seconds must be between 1 and 600")
    asyncio.run(run(args.pause_file, args.maximum_seconds))
