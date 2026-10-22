#[tokio::main]
async fn main() {
    easycontrol::run().await.expect("easycontrol failed");
}
