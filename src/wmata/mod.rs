pub mod types;
mod util;

use defmt::debug;
use embedded_nal_async::{Dns, TcpConnect};
use heapless::String;
use miniserde::Deserialize;
use reqwless::{
    client::HttpClient,
    request::{Method, RequestBuilder},
};

use crate::wmata::{
    types::{NextTrainsResponse, Station},
    util::build_next_trains_url,
};

pub const USER_AGENT: &str = "esp-wmata-pids";
pub const API: &str = "http://api.wmata.com";

#[derive(Debug)]
pub enum Error {
    Http(reqwless::Error),
    Utf8(core::str::Utf8Error),
    Json(miniserde::Error),
    Format(core::fmt::Error),
}

impl From<reqwless::Error> for Error {
    fn from(value: reqwless::Error) -> Self {
        Self::Http(value)
    }
}

impl From<core::str::Utf8Error> for Error {
    fn from(value: core::str::Utf8Error) -> Self {
        Self::Utf8(value)
    }
}

impl From<miniserde::Error> for Error {
    fn from(value: miniserde::Error) -> Self {
        Self::Json(value)
    }
}

impl From<core::fmt::Error> for Error {
    fn from(value: core::fmt::Error) -> Self {
        Self::Format(value)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Http(e) => write!(f, "http: {:?}", e),
            Error::Utf8(e) => write!(f, "utf8: {}", e),
            Error::Json(_) => write!(f, "json decode failed"),
            Error::Format(e) => write!(f, "fmt: {}", e),
        }
    }
}

impl defmt::Format for Error {
    fn format(&self, f: defmt::Formatter) {
        match self {
            Error::Http(e) => defmt::write!(f, "http: {:?}", e),
            Error::Utf8(e) => defmt::write!(f, "utf8: {:?}", defmt::Display2Format(e)),
            Error::Json(_) => defmt::write!(f, "json decode failed"),
            Error::Format(_) => defmt::write!(f, "fmt error"),
        }
    }
}

/// WMATA Api client as a `reqwless` client wrapper. A WMATA Api key is required.
pub struct Client<'a, T, D>
where
    T: TcpConnect + 'a,
    D: Dns + 'a,
{
    reqwless: HttpClient<'a, T, D>,
    rx_buf: &'a mut [u8],
    api_key: &'a str,
}

impl<'a, T, D> Client<'a, T, D>
where
    T: TcpConnect + 'a,
    D: Dns + 'a,
{
    /// Create a new `WmataClient` around a `reqwless` client.
    /// Takes ownership of the reqwless client.
    /// The Api key is required.
    pub fn new(reqwless: HttpClient<'a, T, D>, rx_buf: &'a mut [u8], api_key: &'a str) -> Self {
        Self {
            reqwless,
            rx_buf,
            api_key,
        }
    }

    /// Convenience function for making requests
    async fn fetch<J: Deserialize>(&mut self, url: &str) -> Result<J, Error> {
        let headers = [("Api_key", self.api_key), ("User-Agent", "esp-wmata-pids")];
        let mut req = self
            .reqwless
            .request(Method::GET, url)
            .await?
            .headers(&headers);

        let res = req.send(self.rx_buf).await?;
        let body = res.body().read_to_end().await?;
        let json = core::str::from_utf8(body)?;
        debug!("{:?}", json);
        miniserde::json::from_str(json).map_err(|e| e.into())
    }

    /// Returns next train arrival information for one or more stations.
    /// Will return an empty set of results when no predictions are available.
    /// Use All for the StationCodes parameter to return predictions for all stations.
    /// For terminal stations (e.g.: Greenbelt, Shady Grove, etc.), predictions may be displayed twice.
    /// Some stations have two platforms (e.g.: Gallery Place, Fort Totten, L'Enfant Plaza, and Metro Center).
    /// To retrieve complete predictions for these stations, be sure to pass in both StationCodes.
    /// For trains with no passengers, the DestinationName will be No Passenger.
    /// Next train arrival information is refreshed once every 20 to 30 seconds approximately.
    ///
    /// # Arguments
    ///
    /// * `station` - station code like `B03`.
    pub async fn next_trains(&mut self, station: Station) -> Result<NextTrainsResponse, Error> {
        let mut buf: String<128> = String::new();
        let url = build_next_trains_url(&mut buf, station)?;
        debug!("{:?}", url);
        self.fetch(url).await
    }
}
