use argon2::{
    Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use std::{
    collections::HashSet,
    sync::{Arc, OnceLock},
};
use tokio::sync::Semaphore;

pub fn username(input: &str) -> Option<String> {
    let value = input.trim().to_ascii_lowercase();
    ((3..=32).contains(&value.len())
        && value
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_'))
    .then_some(value)
}

pub fn allowed(password: &str) -> bool {
    static BLOCKLIST: OnceLock<HashSet<String>> = OnceLock::new();
    (15..=128).contains(&password.chars().count())
        && !BLOCKLIST
            .get_or_init(|| {
                include_str!("../data/common-passwords.txt")
                    .lines()
                    .map(str::to_lowercase)
                    .collect()
            })
            .contains(&password.to_lowercase())
}

fn engine() -> Argon2<'static> {
    Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        Params::new(19 * 1024, 2, 1, None).expect("fixed Argon2 parameters"),
    )
}

#[derive(Clone)]
pub struct Passwords {
    workers: Arc<Semaphore>,
    dummy: Arc<String>,
}
impl Passwords {
    pub async fn new() -> Result<Self, ()> {
        let dummy = tokio::task::spawn_blocking(|| {
            engine()
                .hash_password(
                    b"dummy account verification only",
                    &SaltString::generate(&mut OsRng),
                )
                .map(|hash| hash.to_string())
                .map_err(|_| ())
        })
        .await
        .map_err(|_| ())??;
        Ok(Self {
            workers: Arc::new(Semaphore::new(2)),
            dummy: Arc::new(dummy),
        })
    }
    pub async fn hash(&self, password: String) -> Result<String, ()> {
        let permit = self.workers.clone().try_acquire_owned().map_err(|_| ())?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            engine()
                .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
                .map(|hash| hash.to_string())
                .map_err(|_| ())
        })
        .await
        .map_err(|_| ())?
    }
    pub async fn verify(&self, password: String, hash: Option<String>) -> Result<bool, ()> {
        let permit = self.workers.clone().try_acquire_owned().map_err(|_| ())?;
        let known = hash.is_some();
        let hash = hash.unwrap_or_else(|| self.dummy.to_string());
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            let hash = PasswordHash::new(&hash).map_err(|_| ())?;
            Ok(engine().verify_password(password.as_bytes(), &hash).is_ok() && known)
        })
        .await
        .map_err(|_| ())?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn input_policy() {
        assert_eq!(username("  USER_name "), Some("user_name".into()));
        for bad in ["ab", "foo-bar", "üser", "a b", &"a".repeat(33)] {
            assert!(username(bad).is_none());
        }
        assert!(!allowed("short"));
        assert!(!allowed("123456789123456789"));
        assert!(allowed(&"🦀".repeat(128)));
        assert!(!allowed(&"🦀".repeat(129)));
        assert!(allowed(" a long unique passphrase 🦀 "));
    }
    #[tokio::test]
    async fn original_bytes_and_random_salts() {
        let p = Passwords::new().await.unwrap();
        let value = " a unique Unicode phrase 🦀 ";
        let a = p.hash(value.into()).await.unwrap();
        let b = p.hash(value.into()).await.unwrap();
        assert_ne!(a, b);
        assert!(p.verify(value.into(), Some(a.clone())).await.unwrap());
        assert!(!p.verify(value.trim().into(), Some(a)).await.unwrap());
        assert!(!p.verify(value.into(), None).await.unwrap());
        let _one = p.workers.clone().acquire_owned().await.unwrap();
        let _two = p.workers.clone().acquire_owned().await.unwrap();
        assert!(p.hash(value.into()).await.is_err());
        assert!(p.verify(value.into(), None).await.is_err());
    }
}
