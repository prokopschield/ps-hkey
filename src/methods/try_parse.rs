use crate::{Hkey, DOUBLE_HASH_SIZE, DOUBLE_HASH_SIZE_PREFIXED, HASH_SIZE, HASH_SIZE_PREFIXED};

impl Hkey {
    pub fn try_parse(value: impl AsRef<[u8]>) -> crate::Result<Self> {
        let bytes = value.as_ref();

        if bytes.is_empty() {
            return Ok(Self::Empty);
        }

        match (bytes[0], bytes.len()) {
            (_, HASH_SIZE) => Self::try_as_direct(bytes),
            (_, DOUBLE_HASH_SIZE) => Self::try_as_encrypted(bytes),
            (b'D', HASH_SIZE_PREFIXED) => Self::try_as_direct(&bytes[1..]),
            (b'E', DOUBLE_HASH_SIZE_PREFIXED) => Self::try_as_encrypted(&bytes[1..]),
            (b'L', DOUBLE_HASH_SIZE_PREFIXED) => Self::try_as_list_ref(&bytes[1..]),
            (b'[', _) => Self::try_as_list(bytes),
            (b'{', _) => Self::try_as_long(bytes),
            _ => Ok(match std::str::from_utf8(bytes) {
                Ok(str) => Self::from_base64_slice(str)?,
                Err(_) => Self::from_raw(bytes)?,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::Hkey;

    #[test]
    fn empty() -> crate::Result<()> {
        let hkey = Hkey::try_parse("")?;

        assert_eq!(hkey.to_string(), "".to_string());

        Ok(())
    }

    #[test]
    fn empty_variant() -> crate::Result<()> {
        let hkey = Hkey::try_parse("")?;

        assert_eq!(hkey, Hkey::Empty);

        Ok(())
    }
}
