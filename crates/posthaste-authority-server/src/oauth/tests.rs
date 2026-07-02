use super::*;
use base64::Engine;
use jsonwebtoken::jwk::Jwk;
use jsonwebtoken::{encode, EncodingKey, Header};
use posthaste_domain_model::TransportSecurity;

const TEST_RSA_PRIVATE_KEY: &str = r"-----BEGIN PRIVATE KEY-----
MIIEvQIBADANBgkqhkiG9w0BAQEFAASCBKcwggSjAgEAAoIBAQCptW7Vkr5e34U+
tg+ktEDbz7DW+UsAqsLZGl9wgSjp06Y4zyUakTZXfifDaeaCGm/aCy+FCnhdiZ49
zzXcASKqoHOHGd6ap/xdPhIbwF5QZSE6aX2pMGgJ/zMSn9uirfiAQMpDCikZOGf4
9oOay6eW1tzcHDUA95QAVN60nK8FKH1yRD/1F1v6Wu0OsK8ablCyIBXkg7dXXKSF
uyfwgoUGcrgMZDDzC4EomMd7hzSjRcqwqzh9wZeLzkY2/Abz7gyiypY0VtKykTqJ
YNjjtIthj/hZ300Znuzpy9a03wE1eeSKIm10fNQW7ZZ29bk2yBaY5YpqMS6dZLwX
21Hx6qnHAgMBAAECggEAPVN5fkcdcQY/zbYXuBKFH4mRY1XJsy+B4tdDZtHduYWI
mx3L0CpqYzqM3vJNYHVyNu502RQ8A70fyEExOtPUNalupgMErImIyh8MhyfATTgG
Rmfph3KdHgOw7omC4moQkzQWgxxQVrNJ6y8VxqHSaVEylX3B75wHyQjiQ40dN/Te
QttxkTTaHYDUNwvaFEX/jsW1EKCcOaqkqCULmMIpQ3Wz8JMaYD6y9xDMbob3SKCx
SsjqXz/CpcnqdVPr8hUWd3K4M1AG6ZcW4XgjOeaaOJr2N8QUg569AzzHaHzjeHqV
gEXBChP1qBijiNORgMzvwk30GmCWVwQ2XoHhgeNmKQKBgQDuaNFFbcl0c83kOrAh
nu5ie+VPBIz/QRoK1o9E0pjqFVSue9jHbM38uOavvnOB/FFUTzuC/+QzMgN8aAuu
lDDXcmv5eaxuV5BcdPrXnR6/yhzMbgsAq6zV1EMN5iDuwGyo+ZbjVs1g1pTllT71
rF6ZJStDzz7SxAnu0sc60eAe+QKBgQC2OvdM9L9oaS0eMHvK15eE38P9vFgPz1FV
+Cla6ASj1kAROcZfw8+13xjnTWXgAMy83YSwVs150tlUfmh5u4ozqhWeSnKtMfbm
u3CpRLTDf5HBCGE7ZCpkEiMNk2kPVa0QjfQSMJzzyz9cyyy1wR10RaTK5rcMl2eC
hMLNLF1+vwKBgQDKQ7EwNymQG+OU+tmNXJoggb6VIGZC9MeUZF4eZJGJH1m9wqKy
5rOH8pL8jRbQM/IIFkSGKnU/nfHpLRikH2OklZXXjQvmfXGjjzd1j/6TdnSiV8YL
5pp2u2O8Of68sBI/9ai27WDHBKZEdS96HKgRQ8CGAiDpjZpjvP18AK0leQKBgHNJ
CK0J5ZHzgBSqTZa9H+FzAvYiUn/mA6nkrp0RTeYspCmBqItrQJvpwUKLx5iYSO5v
IgPBVorspot60TO6PquCvdx/ct85Td8Y1CRyD/3iVd6OI51EOEFI7B4plPybkjp3
4+IiGRlvCu30p5twyeaGLMQkg8eWfWin/ul4WMnXAoGAeU8NhPQs2A5aCgwyvFiy
b6kcHjMRGGyc0rUmlID7GJDHoBzVs1oHQKyyrCPCKypvw3ZNzntWASN73imjTyV9
bT/1ANJYOasdMeMHJxfTFCa0d2HR6JYy01mtiIgx4SN2u6za/H3xEaq96blpK2fV
TaMgUWVodLXy+lMRbtUQ97M=
-----END PRIVATE KEY-----";

fn signed_id_token(kid: &str, nonce: &str) -> (String, JwkSet) {
    let encoding_key = EncodingKey::from_rsa_pem(TEST_RSA_PRIVATE_KEY.as_bytes()).expect("RSA key");
    let mut header = Header::new(Algorithm::RS256);
    header.kid = Some(kid.to_string());
    let token = encode(
        &header,
        &serde_json::json!({
            "aud": "client-id",
            "email": "user@example.test",
            "email_verified": true,
            "exp": 2000000000,
            "iss": "https://accounts.google.com",
            "nonce": nonce,
        }),
        &encoding_key,
    )
    .expect("signed token");
    let mut jwk = Jwk::from_encoding_key(&encoding_key, Algorithm::RS256).expect("jwk");
    jwk.common.key_id = Some(kid.to_string());
    (token, JwkSet { keys: vec![jwk] })
}

mod flow_store;
mod openid_claim_validation;
mod openid_jwks;
mod provider_profiles;
mod token_session;
