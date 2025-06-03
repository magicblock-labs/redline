use base64::{prelude::BASE64_STANDARD, Engine};
use hash::{Hash, HASH_BYTES};
use json::{JsonValueTrait, LazyValue};

pub fn account_update_extractor(value: LazyValue) -> Option<u64> {
    let value = value.get("value")?;

    let mut iter = value.get("data").and_then(LazyValue::into_array_iter)?;
    let data = iter.next();
    let data = data.as_str()?;
    let data = match iter.next().as_str()? {
        "base58" => bs58::decode(data).into_vec().ok()?,
        "base64" => BASE64_STANDARD.decode(data).ok()?,
        "base64+zstd" => {
            let decoded = BASE64_STANDARD.decode(data).ok()?;
            zstd::decode_all(decoded.as_slice()).ok()?
        }
        _ => return None,
    };
    let mut number = [0; 8];
    number.copy_from_slice(data.get(..8)?);
    Some(u64::from_le_bytes(number))
}

pub fn value_extractor(value: LazyValue) -> Option<bool> {
    Some(value.get("value")?.is_object())
}

// TODO: use in getSignatureStatuses implementation
#[allow(unused)]
pub fn signature_status_extractor_http(value: LazyValue) -> Option<bool> {
    Some(!value.is_null())
}

pub fn signature_response_extractor(value: LazyValue) -> Option<bool> {
    Some(value.as_str().is_some())
}

pub fn signature_status_extractor_ws(value: LazyValue) -> Option<bool> {
    let value = value.get("value")?;

    value.get("err").map(|e| e.is_null())
}

pub fn blockhash_extractor(value: LazyValue) -> Option<Hash> {
    let value = value.get("value")?;

    let blockhash = value.get("blockhash")?;
    let blockhash = blockhash.as_str()?;
    let mut buffer = [0u8; HASH_BYTES];
    bs58::decode(blockhash)
        .onto(&mut buffer)
        .expect("failed to decode blockhash");
    Some(Hash::new_from_array(buffer))
}
