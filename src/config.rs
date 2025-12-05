use embedded_storage::{ReadStorage, Storage};
use esp_storage::{FlashStorage, FlashStorageError};
use heapless::String;
use thiserror::Error;

pub const LEN: usize = core::mem::size_of::<WmataConfig>(); // 136

#[derive(Error, Debug)]
pub enum WmataConfigError {
    #[error("Buffer must be at least length: {}", LEN)]
    BufferTooSmall,
    #[error("Crc checksum failed")]
    BadChecksum,
    #[error("one or more args were too long")]
    BadArgs,
    #[error("Strings must be valid UTF-8")]
    Utf8(#[from] core::str::Utf8Error),
    #[error("flash error: {0:?}")]
    Flash(FlashStorageError),
}

impl From<FlashStorageError> for WmataConfigError {
    fn from(e: FlashStorageError) -> Self {
        Self::Flash(e)
    }
}

#[derive(defmt::Format)]
#[repr(C)]
pub struct WmataConfig {
    pub version: u8,
    ssid_len: u8,
    pass_len: u8,
    api_key_len: u8,
    pub ssid: String<32>,
    pub pass: String<64>,
    pub api_key: String<32>,
    crc: Option<u32>,
}

impl WmataConfig {
    pub fn new(ssid: &str, pass: &str, api_key: &str) -> Result<Self, WmataConfigError> {
        let ssid_len = ssid.len();
        let pass_len = pass.len();
        let api_key_len = api_key.len();

        if ssid_len > 32 || pass_len > 64 || api_key_len > 32 {
            return Err(WmataConfigError::BadArgs);
        }

        let mut new_ssid: String<32> = String::new();
        new_ssid.push_str(ssid).unwrap();

        let mut new_pass: String<64> = String::new();
        new_pass.push_str(pass).unwrap();

        let mut new_api_key: String<32> = String::new();
        new_api_key.push_str(api_key).unwrap();

        Ok(Self {
            version: 1,
            ssid_len: ssid_len as u8,
            pass_len: pass_len as u8,
            api_key_len: api_key_len as u8,
            ssid: new_ssid,
            pass: new_pass,
            api_key: new_api_key,
            crc: None,
        })
    }

    fn to_bytes(&self, bytes: &mut [u8]) -> Result<(), WmataConfigError> {
        if bytes.len() < LEN {
            return Err(WmataConfigError::BufferTooSmall);
        }

        bytes.fill(0);
        {
            let (version_slice, rest) = bytes
                .split_first_mut()
                .ok_or(WmataConfigError::BufferTooSmall)?;
            *version_slice = self.version;

            let (ssid_len_slice, rest) = rest
                .split_first_mut()
                .ok_or(WmataConfigError::BufferTooSmall)?;
            *ssid_len_slice = self.ssid_len;

            let (pass_len_slice, rest) = rest
                .split_first_mut()
                .ok_or(WmataConfigError::BufferTooSmall)?;
            *pass_len_slice = self.pass_len;

            let (api_key_len_slice, rest) = rest
                .split_first_mut()
                .ok_or(WmataConfigError::BufferTooSmall)?;
            *api_key_len_slice = self.api_key_len;

            let (ssid_slice, rest) = rest
                .split_first_chunk_mut::<32>()
                .ok_or(WmataConfigError::BufferTooSmall)?;
            //TODO: this panics because self.ssid.as_bytes() returns only the used portion of the string, so there may be a length mismatch
            ssid_slice.copy_from_slice(self.ssid.as_bytes());

            let (pass_slice, rest) = rest
                .split_first_chunk_mut::<64>()
                .ok_or(WmataConfigError::BufferTooSmall)?;
            pass_slice.copy_from_slice(self.pass.as_bytes());

            let (api_key_slice, rest) = rest
                .split_first_chunk_mut::<32>()
                .ok_or(WmataConfigError::BufferTooSmall)?;
            api_key_slice.copy_from_slice(self.api_key.as_bytes());

            // this block makes sure we have 4 bytes left for crc
            let (_crc_slice, _rest) = rest
                .split_first_chunk_mut::<4>()
                .ok_or(WmataConfigError::BufferTooSmall)?;
        }
        let crc = crc32fast::hash(&bytes[..LEN - 4]); // crc.len() = 4
        bytes[LEN - 4..LEN].copy_from_slice(&crc.to_le_bytes());

        Ok(())
    }

    fn from_bytes(bytes: &mut [u8]) -> Result<Self, WmataConfigError> {
        let (version_slice, rest) = bytes
            .split_first_mut()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let version = *version_slice;

        let (ssid_len_slice, rest) = rest
            .split_first_mut()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let ssid_len = *ssid_len_slice;

        let (pass_len_slice, rest) = rest
            .split_first_mut()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let pass_len = *pass_len_slice;

        let (api_key_len_slice, rest) = rest
            .split_first_mut()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let api_key_len = *api_key_len_slice;

        let (ssid_slice, rest) = rest
            .split_first_chunk_mut::<32>()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let mut ssid: String<32> = String::new();
        ssid.push_str(core::str::from_utf8(ssid_slice)?).unwrap();

        let (pass_slice, rest) = rest
            .split_first_chunk_mut::<64>()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let mut pass: String<64> = String::new();
        pass.push_str(core::str::from_utf8(pass_slice)?).unwrap();

        let (api_key_slice, rest) = rest
            .split_first_chunk_mut::<32>()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let mut api_key: String<32> = String::new();
        api_key
            .push_str(core::str::from_utf8(api_key_slice)?)
            .unwrap();

        let (crc_slice, _rest) = rest
            .split_first_chunk_mut::<4>()
            .ok_or(WmataConfigError::BufferTooSmall)?;
        let crc = u32::from_le_bytes(*crc_slice);

        if crc32fast::hash(&bytes[..LEN - 4]) != crc {
            return Err(WmataConfigError::BadChecksum);
        }

        let config = Self {
            version,
            ssid_len,
            pass_len,
            api_key_len,
            ssid,
            pass,
            api_key,
            crc: Some(crc),
        };

        Ok(config)
    }

    pub fn ssid(&self) -> &str {
        self.ssid.as_str()
    }

    pub fn pass(&self) -> &str {
        self.pass.as_str()
    }

    pub fn api_key(&self) -> &str {
        self.api_key.as_str()
    }

    pub fn save(&self, flash: &mut FlashStorage) -> Result<(), FlashStorageError> {
        let mut bytes = [0u8; LEN];
        self.to_bytes(&mut bytes).unwrap();
        let offset = flash.capacity() as u32 - FlashStorage::SECTOR_SIZE;
        flash.write(offset, &bytes)?;

        Ok(())
    }

    pub fn load(flash: &mut FlashStorage) -> Result<Self, WmataConfigError> {
        let mut bytes = [0u8; LEN];
        let offset = flash.capacity() as u32 - FlashStorage::SECTOR_SIZE;
        flash.read(offset, &mut bytes)?;

        Self::from_bytes(&mut bytes)
    }
}
