use crate::Hkey;

impl Hkey {
    #[must_use]
    pub fn parse(value: impl AsRef<[u8]>) -> Self {
        let bytes = value.as_ref();

        Self::try_parse(bytes).unwrap_or_else(|_| Self::from_base64_slice(bytes))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ps_base64::base64;
    use ps_hash::{hash, Hash};

    use super::*;

    // ------------------------
    // Helpers
    // ------------------------

    fn arcstr(s: &str) -> Arc<str> {
        Arc::<str>::from(s)
    }

    fn mk_hash(data: impl AsRef<[u8]>) -> Arc<ps_hash::Hash> {
        Arc::new(hash(data).expect("hashing failed"))
    }

    fn canonize_base64(str: impl AsRef<[u8]>) -> String {
        let str = str.as_ref();
        let raw = base64::decode(str);
        let mut b64 = base64::encode(&raw);

        b64.truncate(str.len());

        b64
    }

    fn canonize_hash(hash: impl AsRef<[u8]>) -> Hash {
        Hash::validate(hash.as_ref()).unwrap()
    }

    fn canonize_hkey(h: Hkey) -> Hkey {
        match h {
            Hkey::Base64(value) => Hkey::Base64(canonize_base64(value.as_bytes()).into()),
            Hkey::Raw(_) => Hkey::parse(h.to_string()),

            Hkey::Direct(hash) => Hkey::Direct(Arc::new(canonize_hash(&**hash))),

            Hkey::Encrypted(hash, key) => {
                Hkey::Encrypted(canonize_hash(&**hash).into(), canonize_hash(&**key).into())
            }

            Hkey::ListRef(hash, key) => {
                Hkey::ListRef(canonize_hash(&**hash).into(), canonize_hash(&**key).into())
            }

            Hkey::List(hkeys) => {
                let hkeys: Vec<Hkey> = hkeys
                    .iter()
                    .map(|hkey| canonize_hkey(hkey.clone()))
                    .collect();

                Hkey::List(hkeys.into())
            }

            hkey => hkey, // are always canonical
        }
    }

    // Format -> Parse once (the "canonicalization step").
    fn canonicalize_once(h: Hkey) -> (Hkey, String) {
        let h1 = canonize_hkey(h);
        let s1 = h1.to_string();

        (h1, s1)
    }

    // Assert that after one canonicalization step, the Hkey/string stabilize
    // over further rounds (idempotence).
    fn assert_stable_after_first_canonicalization(h: Hkey) -> Hkey {
        let (c1, s1) = canonicalize_once(h);
        let c2 = Hkey::parse(s1.as_bytes());
        assert_eq!(
            c1, c2,
            "Canonicalized Hkey must be stable over a second parse"
        );
        let s2 = String::from(&c2);
        assert_eq!(
            s1, s2,
            "Canonicalized string must be stable over a second format"
        );
        c1
    }

    // Produce deterministic byte patterns to exercise encoding.
    fn bytes_pattern(len: usize, seed: u8) -> Arc<[u8]> {
        let mut v = Vec::with_capacity(len);
        for i in 0..len {
            let b = seed
                .wrapping_mul(31)
                .wrapping_add((i as u8).wrapping_mul(17))
                ^ 0xA5;
            v.push(b);
        }
        v.into()
    }

    fn alternating(len: usize, a: u8, b: u8) -> Arc<[u8]> {
        let mut v = Vec::with_capacity(len);
        for i in 0..len {
            v.push(if i % 2 == 0 { a } else { b });
        }
        v.into()
    }

    // Create non-canonical Base64 spellings from a canonical Hkey-base64 string.
    // We do not assume the exact Hkey alphabet; we only build alternative encodings
    // by adding padding and MIME-style line breaks/whitespace, and a best-effort
    // mapping to the "standard" base64 alphabet (+,/).
    fn pad_to_multiple_of_4(mut s: String) -> String {
        let pad = (4 - (s.len() % 4)) % 4;
        for _ in 0..pad {
            s.push('=');
        }
        s
    }

    fn insert_line_breaks(s: &str, width: usize) -> String {
        let mut out = String::with_capacity(s.len() + s.len() / width + 8);
        for (i, ch) in s.chars().enumerate() {
            if i > 0 && i % width == 0 {
                out.push('\n'); // MIME-style LF is sufficient for the test
            }
            out.push(ch);
        }
        out
    }

    fn insert_spaces_every(s: &str, every: usize) -> String {
        let mut out = String::with_capacity(s.len() + s.len() / every);
        for (i, ch) in s.chars().enumerate() {
            if i > 0 && i % every == 0 {
                out.push(' ');
            }
            out.push(ch);
        }
        out
    }

    // Convert a canonical "Hkey base64" to a likely "standard" alphabet version.
    // This is heuristic: if Hkey already uses the standard alphabet, this may
    // produce identical output, which is still fine because we also add padding.
    fn to_std_alphabet(mut s: String) -> String {
        s = s.replace('-', "+");
        s = s.replace('_', "/");
        s
    }

    // Build a set of non-canonical variants spelling the same payload bytes.
    fn alt_base64_spellings(canonical: &str) -> Vec<String> {
        let mut alts = Vec::new();

        // Add padding (common outside of "unpadded" encodings).
        alts.push(pad_to_multiple_of_4(canonical.to_string()));

        // MIME: padded with line breaks.
        let padded = pad_to_multiple_of_4(canonical.to_string());
        alts.push(insert_line_breaks(&padded, 8));

        // MIME-ish: inject spaces (some decoders ignore whitespace).
        alts.push(insert_spaces_every(&padded, 5));

        // Try "standard" alphabet (+,/), with padding.
        let std_alpha = to_std_alphabet(canonical.to_string());
        alts.push(pad_to_multiple_of_4(std_alpha));

        // Leading/trailing whitespace around the whole token.
        alts.push(format!(
            "  {}\n",
            pad_to_multiple_of_4(canonical.to_string())
        ));

        alts
    }

    // ------------------------
    // Raw -> Base64 canonicalization
    // ------------------------

    fn assert_raw_canonicalizes_to_base64(raw: Arc<[u8]>) {
        let expected = ps_base64::encode(&*raw);
        let canon = assert_stable_after_first_canonicalization(Hkey::Raw(raw));
        match canon {
            Hkey::Base64(s) => {
                assert_eq!(
                    s.as_ref(),
                    expected,
                    "Raw must canonicalize to Base64 with Hkey alphabet"
                );
            }
            other => panic!("Raw must canonicalize to Base64, got {:?}", other),
        }
    }

    #[test]
    fn raw_len0_and_all_len1_canonicalize_to_base64() {
        // len = 0
        assert_raw_canonicalizes_to_base64(Arc::<[u8]>::default());

        // len = 1, exhaustive over all 256 possible bytes
        for b in 0u16..=255 {
            assert_raw_canonicalizes_to_base64(vec![b as u8].into());
        }
    }

    #[test]
    fn raw_patterns_canonicalize_to_base64() {
        let seeds: [u8; 6] = [0, 1, 7, 63, 127, 255];
        let lengths: [usize; 10] = [2, 3, 4, 5, 7, 8, 15, 16, 31, 64];

        for &len in &lengths {
            for &seed in &seeds {
                assert_raw_canonicalizes_to_base64(bytes_pattern(len, seed));
            }
        }

        // Alternating / extreme bytes
        for &len in &[2usize, 8, 16, 32, 64, 128, 255] {
            assert_raw_canonicalizes_to_base64(vec![0x00; len].into());
            assert_raw_canonicalizes_to_base64(vec![0xFF; len].into());
            assert_raw_canonicalizes_to_base64(alternating(len, 0x00, 0xFF));
            assert_raw_canonicalizes_to_base64(alternating(len, 0xAA, 0x55));
        }

        // Longish
        for &len in &[257usize, 512, 1024] {
            assert_raw_canonicalizes_to_base64(bytes_pattern(len, 0x6D));
            assert_raw_canonicalizes_to_base64(alternating(len, 0x13, 0xE7));
        }
    }

    // Ensure that even if the canonical base64 string begins with 'E' or 'L',
    // parsing chooses Base64 (not Encrypted/ListRef). We craft raw payloads
    // whose base64 will start with those letters.
    #[test]
    fn raw_canonical_base64_begins_with_e_or_l_is_still_base64() {
        // First base64 char roughly depends on (first_byte >> 2).
        // Values chosen to bias toward 'E' (~index 4) and 'L' (~index 11).
        for &first in &[16u8, 44u8] {
            let raw: Arc<[u8]> = vec![first, 0, 0].into();
            let expected = ps_base64::encode(&*raw);
            let canon = assert_stable_after_first_canonicalization(Hkey::Raw(raw));
            match canon {
                Hkey::Base64(s) => assert_eq!(s.as_ref(), expected),
                other => panic!("Expected Base64, got {:?}", other),
            }
        }
    }

    // ------------------------
    // Base64 canonicalization (other encodings -> canonical Hkey alphabet)
    // ------------------------

    fn base64_bytes_examples() -> Vec<Arc<[u8]>> {
        vec![
            Arc::<[u8]>::from(b"" as &[u8]),
            Arc::<[u8]>::from(b"f" as &[u8]),
            Arc::<[u8]>::from(b"fo" as &[u8]),
            Arc::<[u8]>::from(b"foo" as &[u8]),
            Arc::<[u8]>::from(b"foob" as &[u8]),
            Arc::<[u8]>::from(b"fooba" as &[u8]),
            Arc::<[u8]>::from(b"foobar" as &[u8]),
            Arc::<[u8]>::from(&[0x00, 0x01, 0x02][..]),
            Arc::<[u8]>::from(&[0xFF, 0xFF, 0xFF][..]),
            bytes_pattern(17, 0x5A),
            bytes_pattern(32, 0xC3),
        ]
    }

    #[test]
    fn base64_canonical_strings_are_stable() {
        for data in base64_bytes_examples() {
            let canonical = ps_base64::encode(&*data);
            let h = Hkey::Base64(arcstr(&canonical));
            let canon = assert_stable_after_first_canonicalization(h);
            match canon {
                Hkey::Base64(s) => assert_eq!(
                    s.as_ref(),
                    canonical,
                    "Canonical Base64 should remain unchanged"
                ),
                other => panic!("Base64 should remain Base64, got {:?}", other),
            }
        }
    }

    #[test]
    fn base64_non_canonical_spellings_normalize_to_canonical() {
        for data in base64_bytes_examples() {
            let canonical = ps_base64::encode(&*data);
            for alt in alt_base64_spellings(&canonical) {
                let h = Hkey::Base64(arcstr(&alt));
                let canon = assert_stable_after_first_canonicalization(h);
                match canon {
                    Hkey::Base64(s) => assert_eq!(
                        s.as_ref(),
                        canonical,
                        "Non-canonical Base64 must normalize to Hkey alphabet: {s} vs. {canonical}"
                    ),
                    other => panic!("Base64 should remain Base64, got {:?}", other),
                }
            }
        }
    }

    // ------------------------
    // Direct / Encrypted / ListRef should already be canonical and stable
    // ------------------------

    #[test]
    fn direct_is_stable() {
        let inputs = &[
            b"" as &[u8],
            b"a",
            b"abc",
            b"123456",
            b"deadbeef",
            b"CAFEBABE",
            b"with-dash_underscore",
        ];
        for &inp in inputs {
            let h = Hkey::Direct(mk_hash(inp));
            let canon = assert_stable_after_first_canonicalization(h.clone());
            assert_eq!(canon, h, "Direct should remain identical");
        }

        // A few more hash inputs from patterns
        for &len in &[1usize, 7, 8, 15, 16, 31, 32] {
            let d = bytes_pattern(len, 0xC3);
            let h = Hkey::Direct(mk_hash(&*d));
            let canon = assert_stable_after_first_canonicalization(h.clone());
            assert_eq!(canon, h, "Direct should remain identical");
        }
    }

    #[test]
    fn encrypted_is_stable() {
        let cases = vec![
            (mk_hash(b"hash-1"), mk_hash(b"key-1")),
            (mk_hash(b"hash-2"), mk_hash(b"key-2")),
        ];
        for (hh, kk) in cases {
            let h = Hkey::Encrypted(hh.clone(), kk.clone());
            let canon = assert_stable_after_first_canonicalization(h.clone());
            assert_eq!(canon, h, "Encrypted should remain identical");
        }

        // Identical components edge case
        let h = mk_hash(b"same");
        let e = Hkey::Encrypted(h.clone(), h.clone());
        let canon = assert_stable_after_first_canonicalization(e.clone());
        assert_eq!(canon, e, "Encrypted identical parts should remain same");
    }

    #[test]
    fn listref_is_stable() {
        let cases = vec![
            (mk_hash(b"list-hash-1"), mk_hash(b"list-key-1")),
            (mk_hash(b"list-hash-2"), mk_hash(b"list-key-2")),
        ];
        for (hh, kk) in cases {
            let h = Hkey::ListRef(hh.clone(), kk.clone());
            let canon = assert_stable_after_first_canonicalization(h.clone());
            assert_eq!(canon, h, "ListRef should remain identical");
        }

        // Identical components edge case
        let h = mk_hash(b"same-lr");
        let lr = Hkey::ListRef(h.clone(), h.clone());
        let canon = assert_stable_after_first_canonicalization(lr.clone());
        assert_eq!(canon, lr, "ListRef identical parts should remain same");
    }

    // ------------------------
    // Lists: Raw -> Base64 inside lists; Base64 non-canonical -> canonical; stable thereafter
    // ------------------------

    #[test]
    fn list_canonicalization_and_stability() {
        // Prepare raw data whose canonical base64 we will know.
        let raw_a: Arc<[u8]> = vec![0, 1, 2, 3].into();
        let raw_b: Arc<[u8]> = bytes_pattern(7, 0x11);

        let b64_raw_a = ps_base64::encode(&*raw_a);
        let b64_raw_b = ps_base64::encode(&*raw_b);

        // Prepare a non-canonical Base64 spelling for "Hello".
        let b_hello: Arc<[u8]> = Arc::<[u8]>::from(b"Hello" as &[u8]);
        let canon_hello = ps_base64::encode(&*b_hello);
        let mime_hello = insert_line_breaks(&pad_to_multiple_of_4(canon_hello.clone()), 3);

        // Construct a list with a mix:
        // - Raw (should become Base64)
        // - Base64 non-canonical (should normalize)
        // - Direct / Encrypted / ListRef (should stay identical)
        let lst = Hkey::List(
            vec![
                Hkey::Raw(raw_a.clone()),
                Hkey::Base64(arcstr(&mime_hello)),
                Hkey::Direct(mk_hash(b"dir-x")),
                Hkey::Encrypted(mk_hash(b"eh"), mk_hash(b"ek")),
                Hkey::ListRef(mk_hash(b"lh"), mk_hash(b"lk")),
                Hkey::Raw(raw_b.clone()),
            ]
            .into(),
        );

        let canon = assert_stable_after_first_canonicalization(lst);

        // Build the expected canonicalized list explicitly
        let expected = Hkey::List(
            vec![
                Hkey::Base64(arcstr(&b64_raw_a)),
                Hkey::Base64(arcstr(&canon_hello)),
                Hkey::Direct(mk_hash(b"dir-x")),
                Hkey::Encrypted(mk_hash(b"eh"), mk_hash(b"ek")),
                Hkey::ListRef(mk_hash(b"lh"), mk_hash(b"lk")),
                Hkey::Base64(arcstr(&b64_raw_b)),
            ]
            .into(),
        );

        assert_eq!(
            canon, expected,
            "List must canonicalize members as specified (Raw->Base64; Base64->Hkey alphabet)"
        );

        // Idempotence is already validated inside assert_stable_after_first_canonicalization.
    }

    // ------------------------
    // Multi-cycle robustness: everything stabilizes after first pass
    // ------------------------

    #[test]
    fn multi_cycle_idempotence_after_first_pass() {
        let samples: Vec<Hkey> = vec![
            // Raw and Base64 non-canonical
            Hkey::Raw(vec![0x00, 0xFF, 0x7E, 0x81].into()),
            {
                let canon = ps_base64::encode(b"foobar");
                let alt = pad_to_multiple_of_4(canon.clone());
                Hkey::Base64(arcstr(&alt))
            },
            // Direct / Encrypted / ListRef
            Hkey::Direct(mk_hash(b"E123notEncrypted")),
            Hkey::Encrypted(mk_hash(b"hash-iter-a"), mk_hash(b"key-iter-a")),
            Hkey::ListRef(mk_hash(b"hash-iter-b"), mk_hash(b"key-iter-b")),
            // Mixed list
            Hkey::List(
                vec![
                    Hkey::Direct(mk_hash(b"x")),
                    Hkey::Raw(vec![1, 2, 3].into()),
                    Hkey::Base64(arcstr(&insert_line_breaks(
                        &pad_to_multiple_of_4(ps_base64::encode(b"Hello")),
                        2,
                    ))),
                    Hkey::Encrypted(mk_hash(b"h-c"), mk_hash(b"k-c")),
                ]
                .into(),
            ),
        ];

        for h in samples {
            let (mut c, mut s) = canonicalize_once(h);
            for _ in 0..5 {
                // Subsequent cycles must be perfectly stable
                let c2 = Hkey::parse(s.as_bytes());
                assert_eq!(c, c2, "Hkey must remain stable across cycles");
                let s2 = String::from(&c2);
                assert_eq!(s, s2, "String must remain stable across cycles");
                c = c2;
                s = s2;
            }
        }
    }
}
