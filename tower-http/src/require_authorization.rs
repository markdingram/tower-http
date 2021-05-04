//! Authorize requests using the [`Authorization`] header.
//!
//! [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
//!
//! # Example
//!
//! ```
//! use tower_http::require_authorization::RequireAuthorizationLayer;
//! use hyper::{Request, Response, Body, Error};
//! use http::{StatusCode, header::AUTHORIZATION};
//! use tower::{Service, ServiceExt, ServiceBuilder, service_fn};
//!
//! async fn handle(request: Request<Body>) -> Result<Response<Body>, Error> {
//!     Ok(Response::new(Body::empty()))
//! }
//!
//! # #[tokio::main]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut service = ServiceBuilder::new()
//!     // Require the `Authorization` header to be `Bearer passwordlol`
//!     .layer(RequireAuthorizationLayer::bearer("passwordlol"))
//!     .service(service_fn(handle));
//!
//! // Requests with the correct token are allowed through
//! let request = Request::builder()
//!     .header(AUTHORIZATION, "Bearer passwordlol")
//!     .body(Body::empty())
//!     .unwrap();
//!
//! let response = service
//!     .ready_and()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::OK, response.status());
//!
//! // Requests with invalid token get a `401 Unauthorized` response
//! let request = Request::builder()
//!     .body(Body::empty())
//!     .unwrap();
//!
//! let response = service
//!     .ready_and()
//!     .await?
//!     .call(request)
//!     .await?;
//!
//! assert_eq!(StatusCode::UNAUTHORIZED, response.status());
//! # Ok(())
//! # }
//! ```

use http::{
    header::{self, HeaderValue},
    Request, Response, StatusCode,
};
use http_body::Body;
use pin_project::pin_project;
use std::{
    fmt,
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};
use tower_layer::Layer;
use tower_service::Service;

/// Layer that applies [`RequireAuthorization`] which authorizes all requests using the
/// [`Authorization`] header.
///
/// See the [module docs](crate::require_authorization) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Debug, Clone)]
pub struct RequireAuthorizationLayer<T> {
    auth: T,
}

impl<ResBody> RequireAuthorizationLayer<Bearer<ResBody>> {
    /// Authorize requests using a "bearer token". Commonly used for OAuth 2.
    ///
    /// The `Authorization` header is required to be `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [`HeaderValue`](http::header::HeaderValue).
    pub fn bearer(token: &str) -> Self
    where
        ResBody: Body + Default,
    {
        Self::custom(Bearer::new(token))
    }
}

impl<ResBody> RequireAuthorizationLayer<Basic<ResBody>> {
    /// Authorize requests using a username and password pair.
    ///
    /// The `Authorization` header is required to be `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    pub fn basic(username: &str, password: &str) -> Self
    where
        ResBody: Body + Default,
    {
        Self::custom(Basic::new(username, password))
    }
}

impl<T> RequireAuthorizationLayer<T>
where
    T: AuthorizeRequest,
{
    /// Authorize requests using a custom scheme.
    ///
    /// The `Authorization` header is required to have the value provided.
    pub fn custom(auth: T) -> RequireAuthorizationLayer<T> {
        Self { auth }
    }
}

impl<S, T> Layer<S> for RequireAuthorizationLayer<T>
where
    T: Clone,
{
    type Service = RequireAuthorization<S, T>;

    fn layer(&self, inner: S) -> Self::Service {
        RequireAuthorization::new(inner, self.auth.clone())
    }
}

/// Middleware that authorizes all requests using the [`Authorization`] header.
///
/// See the [module docs](crate::require_authorization) for an example.
///
/// [`Authorization`]: https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Authorization
#[derive(Clone, Debug)]
pub struct RequireAuthorization<S, T> {
    inner: S,
    auth: T,
}

impl<S, T> RequireAuthorization<S, T> {
    fn new(inner: S, auth: T) -> Self {
        Self { inner, auth }
    }

    define_inner_service_accessors!();
}

impl<S, ResBody> RequireAuthorization<S, Bearer<ResBody>> {
    /// Authorize requests using a "bearer token". Commonly used for OAuth 2.
    ///
    /// The `Authorization` header is required to be `Bearer {token}`.
    ///
    /// # Panics
    ///
    /// Panics if the token is not a valid [`HeaderValue`](http::header::HeaderValue).
    pub fn bearer(inner: S, token: &str) -> Self
    where
        ResBody: Body + Default,
    {
        Self::custom(inner, Bearer::new(token))
    }
}

impl<S, ResBody> RequireAuthorization<S, Basic<ResBody>> {
    /// Authorize requests using a username and password pair.
    ///
    /// The `Authorization` header is required to be `Basic {credentials}` where `credentials` is
    /// `base64_encode("{username}:{password}")`.
    ///
    /// Since the username and password is sent in clear text it is recommended to use HTTPS/TLS
    /// with this method. However use of HTTPS/TLS is not enforced by this middleware.
    pub fn basic(inner: S, username: &str, password: &str) -> Self
    where
        ResBody: Body + Default,
    {
        Self::custom(inner, Basic::new(username, password))
    }
}

impl<S, T> RequireAuthorization<S, T>
where
    T: AuthorizeRequest,
{
    /// Authorize requests using a custom scheme.
    ///
    /// The `Authorization` header is required to have the value provided.
    pub fn custom(inner: S, auth: T) -> RequireAuthorization<S, T> {
        Self { inner, auth }
    }
}

impl<ReqBody, ResBody, S, T> Service<Request<ReqBody>> for RequireAuthorization<S, T>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
    T: AuthorizeRequest<ResponseBody = ResBody>,
{
    type Response = Response<ResBody>;
    type Error = S::Error;
    type Future = ResponseFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        if let Some(output) = self.auth.authorize(&req) {
            self.auth.on_authorized(&mut req, output);

            ResponseFuture {
                kind: Kind::Future(self.inner.call(req)),
            }
        } else {
            let res = self.auth.unauthorized_response(&req);
            ResponseFuture {
                kind: Kind::Error(Some(res)),
            }
        }
    }
}

/// Response future for [`RequireAuthorization`].
#[pin_project]
pub struct ResponseFuture<F, B> {
    #[pin]
    kind: Kind<F, B>,
}

#[pin_project(project = KindProj)]
enum Kind<F, B> {
    Future(#[pin] F),
    Error(Option<Response<B>>),
}

impl<F, B, E> Future for ResponseFuture<F, B>
where
    F: Future<Output = Result<Response<B>, E>>,
{
    type Output = F::Output;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project().kind.project() {
            KindProj::Future(future) => future.poll(cx),
            KindProj::Error(response) => {
                let response = response.take().unwrap();
                Poll::Ready(Ok(response))
            }
        }
    }
}

/// Trait for authorizing requests.
pub trait AuthorizeRequest {
    /// The output type of doing the authorization.
    ///
    /// Use `()` if authorization doesn't produce any meaningful output.
    type Output;

    /// The body type used for responses to unauthorized requests.
    type ResponseBody: Body;

    /// Authorize the request.
    ///
    /// If `Some(_)` is returned then the request is allowed through, otherwise not.
    fn authorize<B>(&mut self, request: &Request<B>) -> Option<Self::Output>;

    /// Callback for when a request has been successfully authorized.
    ///
    /// For example this allows you to save `Self::Output` in a [request extension][] to make it
    /// available to services further down the stack.
    ///
    /// Defaults to doing nothing.
    ///
    /// [request extension]: https://docs.rs/http/0.2.4/http/struct.Extensions.html
    fn on_authorized<B>(&mut self, _request: &mut Request<B>, _output: Self::Output) {}

    /// Create the response for an unauthorized request.
    fn unauthorized_response<B>(&mut self, request: &Request<B>) -> Response<Self::ResponseBody>;
}

/// Type that performs "bearer token" authorization.
///
/// See [`RequireAuthorization::bearer`] for more details.
pub struct Bearer<ResBody> {
    header_value: HeaderValue,
    _ty: PhantomData<fn() -> ResBody>,
}

impl<ResBody> Bearer<ResBody> {
    fn new(token: &str) -> Self
    where
        ResBody: Body + Default,
    {
        Self {
            header_value: format!("Bearer {}", token)
                .parse()
                .expect("token is not a valid header value"),
            _ty: PhantomData,
        }
    }
}

impl<ResBody> Clone for Bearer<ResBody> {
    fn clone(&self) -> Self {
        Self {
            header_value: self.header_value.clone(),
            _ty: PhantomData,
        }
    }
}

impl<ResBody> fmt::Debug for Bearer<ResBody> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Bearer")
            .field("header_value", &self.header_value)
            .finish()
    }
}

impl<ResBody> AuthorizeRequest for Bearer<ResBody>
where
    ResBody: Body + Default,
{
    type Output = ();
    type ResponseBody = ResBody;

    fn authorize<B>(&mut self, request: &Request<B>) -> Option<Self::Output> {
        if let Some(actual) = request.headers().get(header::AUTHORIZATION) {
            (actual == self.header_value).then(|| ())
        } else {
            None
        }
    }

    fn unauthorized_response<B>(&mut self, _request: &Request<B>) -> Response<Self::ResponseBody> {
        let body = ResBody::default();
        let mut res = Response::new(body);
        *res.status_mut() = StatusCode::UNAUTHORIZED;
        res
    }
}

/// Type that performs basic authorization.
///
/// See [`RequireAuthorization::basic`] for more details.
pub struct Basic<ResBody> {
    header_value: HeaderValue,
    _ty: PhantomData<fn() -> ResBody>,
}

impl<ResBody> Basic<ResBody> {
    fn new(username: &str, password: &str) -> Self
    where
        ResBody: Body + Default,
    {
        let encoded = base64::encode(format!("{}:{}", username, password));
        let header_value = format!("Basic {}", encoded).parse().unwrap();
        Self {
            header_value,
            _ty: PhantomData,
        }
    }
}

impl<ResBody> Clone for Basic<ResBody> {
    fn clone(&self) -> Self {
        Self {
            header_value: self.header_value.clone(),
            _ty: PhantomData,
        }
    }
}

impl<ResBody> fmt::Debug for Basic<ResBody> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Basic")
            .field("header_value", &self.header_value)
            .finish()
    }
}

impl<ResBody> AuthorizeRequest for Basic<ResBody>
where
    ResBody: Body + Default,
{
    type Output = ();
    type ResponseBody = ResBody;

    fn authorize<B>(&mut self, request: &Request<B>) -> Option<Self::Output> {
        if let Some(actual) = request.headers().get(header::AUTHORIZATION) {
            (actual == self.header_value).then(|| ())
        } else {
            None
        }
    }

    fn unauthorized_response<B>(&mut self, _request: &Request<B>) -> Response<Self::ResponseBody> {
        let body = ResBody::default();
        let mut res = Response::new(body);
        *res.status_mut() = StatusCode::UNAUTHORIZED;
        res.headers_mut()
            .insert(header::WWW_AUTHENTICATE, "Basic".parse().unwrap());
        res
    }
}