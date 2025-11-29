use core::fmt::Write;
use heapless::String;

use crate::wmata::{API, types::Station};

/// We can't use `format!()` so we're stuck with this
pub(super) fn build_next_trains_url(
    buf: &mut String<128>,
    station: Station,
) -> Result<&str, core::fmt::Error> {
    buf.clear();
    write!(
        buf,
        "{API}/StationPrediction.svc/json/GetPrediction/{}",
        station.code()
    )?;

    Ok(buf)
}
