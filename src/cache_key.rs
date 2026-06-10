use serde::Serialize;
use sha2::{Digest, Sha256};

pub const CANONICALIZATION_VERSION: u32 = 1;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HashPreimage<'a, T: ?Sized> {
    domain: &'a str,
    payload: &'a T,
}

pub fn canonical_json_bytes<T>(value: &T) -> anyhow::Result<Vec<u8>>
where
    T: Serialize + ?Sized,
{
    Ok(serde_jcs::to_vec(value)?)
}

pub fn hash_json<T>(domain: &str, payload: &T) -> anyhow::Result<String>
where
    T: Serialize + ?Sized,
{
    let preimage = HashPreimage { domain, payload };
    Ok(hex_sha256(&canonical_json_bytes(&preimage)?))
}

pub fn hex_sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn canonical_json_matches_rfc_8785_key_ordering_and_escaping() {
        let value = json!({
            "z": "€\u{000f}\n",
            "a": [true, null, false],
        });

        let canonical = String::from_utf8(canonical_json_bytes(&value).unwrap()).unwrap();

        assert_eq!(canonical, r#"{"a":[true,null,false],"z":"€\u000f\n"}"#);
    }

    #[test]
    fn sha256_hex_has_stable_golden_vector() {
        let canonical = canonical_json_bytes(&json!({"b": 2, "a": 1})).unwrap();

        assert_eq!(
            String::from_utf8(canonical.clone()).unwrap(),
            r#"{"a":1,"b":2}"#
        );
        assert_eq!(
            hex_sha256(&canonical),
            "43258cff783fe7036d8a43033f830adfc60ec037382473548ac742b888292777"
        );
    }

    #[test]
    fn hash_json_domains_prevent_cross_type_collisions() {
        let payload = json!({"a": 1, "b": 2});

        assert_ne!(
            hash_json("source-v1", &payload).unwrap(),
            hash_json("config-v1", &payload).unwrap()
        );
    }
}
