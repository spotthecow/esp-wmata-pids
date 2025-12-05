use bincode::{
    Decode, Encode, decode_from_slice, encode_into_slice,
    error::{DecodeError, EncodeError},
};
use embedded_storage::{ReadStorage, Storage};
use esp_storage::{FlashStorage, FlashStorageError};
use thiserror::Error;

pub const CHECKSUM_SZ: usize = core::mem::size_of::<u32>();
pub const SSID_MAX_LEN: usize = 32;
pub const PASS_MAX_LEN: usize = 64;
pub const API_KEY_MAX_LEN: usize = 32;
pub const CONFIG_SZ: usize = core::mem::size_of::<Config>() + CHECKSUM_SZ; // 132 + 4 = 136

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Buffer must be at least length: {}", CONFIG_SZ)]
    BufferTooSmall,
    #[error("Crc checksum failed")]
    BadChecksum,
    #[error("one or more args were too long")]
    BadArgs,
    #[error("flash error: {0:?}")]
    Flash(FlashStorageError),
    #[error("decode error: {0:?}")]
    Decode(DecodeError),
    #[error("encode error: {0:?}")]
    Encode(EncodeError),
}

impl defmt::Format for ConfigError {
    fn format(&self, fmt: defmt::Formatter) {
        defmt::write!(fmt, "{}", defmt::Display2Format(self))
    }
}

impl From<FlashStorageError> for ConfigError {
    fn from(e: FlashStorageError) -> Self {
        Self::Flash(e)
    }
}

impl From<DecodeError> for ConfigError {
    fn from(e: DecodeError) -> Self {
        Self::Decode(e)
    }
}

impl From<EncodeError> for ConfigError {
    fn from(e: EncodeError) -> Self {
        Self::Encode(e)
    }
}

#[derive(defmt::Format, Encode, Decode)]
pub struct Config {
    version: u8,
    ssid_len: u8,
    pass_len: u8,
    api_key_len: u8,
    ssid: [u8; SSID_MAX_LEN],
    pass: [u8; PASS_MAX_LEN],
    api_key: [u8; API_KEY_MAX_LEN],
}

impl Config {
    pub fn new(ssid: &str, pass: &str, api_key: &str) -> Result<Self, ConfigError> {
        let ssid_len = ssid.len();
        let pass_len = pass.len();
        let api_key_len = api_key.len();

        if ssid_len > SSID_MAX_LEN || pass_len > PASS_MAX_LEN || api_key_len > API_KEY_MAX_LEN {
            return Err(ConfigError::BadArgs);
        }

        let mut new_ssid = [0u8; SSID_MAX_LEN];
        new_ssid[..ssid_len].copy_from_slice(ssid.as_bytes());

        let mut new_pass = [0u8; PASS_MAX_LEN];
        new_pass[..pass_len].copy_from_slice(pass.as_bytes());

        let mut new_api_key = [0u8; API_KEY_MAX_LEN];
        new_api_key[..api_key_len].copy_from_slice(api_key.as_bytes());

        Ok(Self {
            version: 1,
            ssid_len: ssid_len as u8,
            pass_len: pass_len as u8,
            api_key_len: api_key_len as u8,
            ssid: new_ssid,
            pass: new_pass,
            api_key: new_api_key,
        })
    }

    /// Encode self using `bincode`, prepending with a crc32 checksum, and storing in `buffer`.
    /// # Returns
    /// Number of bytes written to `buffer` (including checksum)
    fn to_bytes(&self, buffer: &mut [u8]) -> Result<usize, ConfigError> {
        if buffer.len() < CONFIG_SZ {
            return Err(ConfigError::BufferTooSmall);
        }

        let (crc32_bytes, payload) = buffer.split_at_mut(CHECKSUM_SZ);
        let len = encode_into_slice(
            self,
            payload,
            bincode::config::standard().with_fixed_int_encoding(),
        )?;
        let crc32 = crc32fast::hash(&payload[..len]);
        crc32_bytes.copy_from_slice(&crc32.to_le_bytes());

        Ok(CHECKSUM_SZ + len)
    }

    fn from_bytes(bytes: &[u8]) -> Result<Self, ConfigError> {
        if bytes.len() < CONFIG_SZ {
            return Err(ConfigError::BufferTooSmall);
        }

        let (crc32_bytes, payload) = bytes.split_at(CHECKSUM_SZ);
        let crc32 = u32::from_le_bytes(crc32_bytes.try_into().unwrap()); // this _should_ be infallible

        if crc32 == crc32fast::hash(payload) {
            Ok(decode_from_slice(
                payload,
                bincode::config::standard().with_fixed_int_encoding(),
            )?
            .0)
        } else {
            Err(ConfigError::BadChecksum)
        }
    }

    pub fn version(&self) -> u8 {
        self.version
    }

    // the following few string accessors just unwrap because they should be valid utf8, since they were passed in as &str originially.
    // unwrap here for simpler call site

    pub fn ssid(&self) -> &str {
        let len = self.ssid_len as usize;
        core::str::from_utf8(&self.ssid[..len]).unwrap()
    }

    pub fn pass(&self) -> &str {
        let len = self.pass_len as usize;
        core::str::from_utf8(&self.pass[..len]).unwrap()
    }

    pub fn api_key(&self) -> &str {
        let len = self.api_key_len as usize;
        core::str::from_utf8(&self.api_key[..len]).unwrap()
    }

    pub fn save(&self, flash: &mut FlashStorage) -> Result<(), ConfigError> {
        let mut bytes = [0u8; CONFIG_SZ];
        self.to_bytes(&mut bytes)?;
        let offset = flash.capacity() as u32 - FlashStorage::SECTOR_SIZE;
        flash.write(offset, &bytes)?;

        Ok(())
    }

    pub fn load(flash: &mut FlashStorage) -> Result<Self, ConfigError> {
        let mut bytes = [0u8; CONFIG_SZ];
        let offset = flash.capacity() as u32 - FlashStorage::SECTOR_SIZE;
        flash.read(offset, &mut bytes)?;

        Self::from_bytes(&bytes)
    }
}
