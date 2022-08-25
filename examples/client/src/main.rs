use axum::{http, body::Bytes};
use http::request;
use hyper::{Request, Body, Response};
use tower::{buffer::Buffer, util::BoxService, BoxError, Service, Layer, ServiceExt};
use tower_http::{map_response_body::MapResponseBodyLayer, trace::TraceLayer, classify::{SharedClassifier, StatusInRangeAsFailures}};
use tracing::info;
use url::Url;

mod body;
// Add `into_stream()` to `http::Body`
use body::BodyStreamExt;
mod error;

pub type Result<T, E = error::ClientError> = std::result::Result<T, E>;

#[derive(Clone)]
pub struct Client {
    inner: Buffer<BoxService<Request<Body>, Response<Body>, BoxError>, Request<Body>>,
    base_url: url::Url,
}

impl Client {
    pub fn new<S, B>(service: S) -> Self
    where
        S: Service<Request<Body>, Response = Response<B>> + Send + 'static,
        S::Future: Send + 'static,
        S::Error: Into<BoxError>,
        B: http_body::Body<Data = Bytes> + Send + 'static,
        B::Error: Into<BoxError>,
    {
        let service = MapResponseBodyLayer::new(|b: B| Body::wrap_stream(b.into_stream()))
            .layer(service)
            .map_err(|e| e.into());

        Self {
            inner: Buffer::new(BoxService::new(service), 1024),
            base_url: Url::parse("http://localhost:3000").unwrap(),
        }
    }

    pub fn absolute_url(&self, path: impl AsRef<str>) -> Result<url::Url> {
        self.base_url
            .join(path.as_ref())
            .map_err(error::ClientError::Url)
    }


    pub async fn post(
        &self,
        route: impl AsRef<str>,
        body: Bytes,
    ) -> Result<()> {
        let builder = self
            .request_builder(self.absolute_url(route)?, http::Method::POST);
            
        let request = builder.body(Body::from(body))?;

        self.execute(request).await?;
        
        Ok(())
    }

    pub async fn get<A>(&self, route: A) -> Result<Bytes>
    where
        A: AsRef<str>,
    {
        let builder = self
            .request_builder(self.absolute_url(route)?, http::Method::GET);

        let response = self.execute(builder.body(Body::empty())?).await?;
        Ok(hyper::body::to_bytes(response.into_body()).await?)
    }

    pub fn request_builder(
        &self,
        url: url::Url,
        method: http::Method,
    ) -> http::request::Builder {
        request::Builder::new()
                .method(method)
                .uri(url.to_string())
    }
    
    pub async fn execute(&self, request: Request<Body>) -> Result<Response<Body>> {
        let mut svc = self.inner.clone();
        svc.ready()
            .await
            .map_err(error::ClientError::Service)?
            .call(request)
            .await
            .map_err(error::ClientError::Service)
    }
}

#[tokio::main]
async fn main() {
    // Setup tracing
    tracing_subscriber::fmt::init();

    let service = tower::ServiceBuilder::new().layer(TraceLayer::new(
        SharedClassifier::new(StatusInRangeAsFailures::new(400..=599))
    )).service(
        hyper::Client::default()
    );

    let client = Client::new(service);

    let route = "foo";
    let body = Bytes::from_static(b"bar");

    info!("Writing to {:?} to {}", body, route);
    client.post(route, body.clone()).await.unwrap();

    info!("Reading from {}", route);
    assert_eq!(client.get(route).await.unwrap(), body);

    info!("Done");
}