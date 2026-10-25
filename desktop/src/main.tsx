import React, { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";
import { App } from "./App";
import { SplashScreen } from "./SplashScreen";
import "./styles.css";
import "./production.css";

function Bootstrap() {
  const [ready, setReady] = useState(false);
  useEffect(() => {
    // A brief brand transition only; never hold the usable workspace hostage.
    const timer = window.setTimeout(() => setReady(true), 850);
    return () => window.clearTimeout(timer);
  }, []);
  return ready ? <App /> : <SplashScreen />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode><Bootstrap /></React.StrictMode>);
