use http_body_util::Full;
use hyper::{body::Bytes, server::conn::http1, service::Service};
use hyper_util::rt::TokioIo;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::time::Duration;
use tokio::net::TcpListener;
use tower::Layer;

type Request = hyper::Request<hyper::body::Incoming>;
type Response = hyper::Response<Full<Bytes>>;

#[derive(Clone)]
struct HelloWorld;

impl Service<Request> for HelloWorld {
    type Response = Response;
    type Error = String;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn call(&self, _req: Request) -> Self::Future {
        let fut = async {
            tokio::time::sleep(Duration::from_secs(5)).await;
            Ok(Response::new(Full::new(Bytes::from("Hello world"))))
        };
        Box::pin(fut)
    }
}

// Wrapper that injects inner service into the layer, then the layer calls inner service
#[derive(Clone)]
pub struct TimeoutLayer {
    duration: Duration,
}

impl TimeoutLayer {
    pub fn new(duration: Duration) -> Self {
        Self { duration }
    }
}

impl<S> Layer<S> for TimeoutLayer {
    type Service = TimeoutService<S>;

    fn layer(&self, service: S) -> Self::Service {
        TimeoutService {
            inner_service: service,
            duration: self.duration,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TimeoutService<S> {
    inner_service: S,
    duration: Duration,
}

impl<S> Service<Request> for TimeoutService<S>
where
    S: Clone + 'static,
    S: Service<Request> + Send + Sync,
    S::Future: Send,
    S::Error: std::fmt::Display,
{
    type Response = S::Response;
    type Error = String;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    fn call(&self, request: Request) -> Self::Future {
        let this = self.clone();

        Box::pin(async move {
            let result =
                tokio::time::timeout(this.duration, this.inner_service.call(request)).await;

            match result {
                Ok(Ok(result)) => Result::<_, Self::Error>::Ok(result),
                Ok(Err(e)) => Result::<Self::Response, _>::Err(e.to_string()),
                Err(_e) => Result::<Self::Response, _>::Err("timedout".to_string()),
            }
        })
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    // We create a TcpListener and bind it to 127.0.0.1:3000
    let listener = TcpListener::bind(addr).await?;

    // We start a loop to continuously accept incoming connections
    loop {
        let (stream, _) = listener.accept().await?;

        let timeout_layer = TimeoutLayer::new(Duration::from_secs(10));
        let timeout_service = timeout_layer.layer(HelloWorld);

        // Use an adapter to access something implementing `tokio::io` traits as if they implement
        // `hyper::rt` IO traits.
        let io = TokioIo::new(stream);

        // Spawn a tokio task to serve multiple connections concurrently
        tokio::task::spawn(async move {
            // Finally, we bind the incoming connection to our `hello` service
            if let Err(err) = http1::Builder::new()
                // `service_fn` converts our function in a `Service`
                .serve_connection(io, timeout_service)
                .await
            {
                println!("Error serving connection: {:?}", err);
            }
        });
    }
}
