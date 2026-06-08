use axum::Router;
use serde_json::Value;
use std::net::SocketAddr;
use tokio::net::TcpListener;

#[allow(dead_code)]
pub async fn spawn_mock_app(app: Router) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

#[allow(dead_code)]
pub fn json_response(value: Value) -> axum::Json<Value> {
    axum::Json(value)
}
