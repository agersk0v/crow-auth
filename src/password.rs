//! Validates password policy and provides bounded Argon2id hashing/verification
//! using a locally provisioned compromised-password list.

use argon2::{
    Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier,
    password_hash::{SaltString, rand_core::OsRng},
};
use std::{collections::HashSet, fmt, path::Path, sync::Arc};
use tokio::sync::Semaphore;

const WORKERS: usize = 4;
const QUEUED: usize = 16;
fn engine() -> Argon2<'static> {
    Argon2::new(
        argon2::Algorithm::Argon2id,
        argon2::Version::V0x13,
        Params::new(19 * 1024, 2, 1, None).expect("fixed Argon2 parameters"),
    )
}

#[derive(Debug, PartialEq, Eq)]
pub enum PasswordError {
    Length,
    Compromised,
    BlocklistUnavailable,
    Overloaded,
    Unavailable,
    InvalidHash,
}
impl fmt::Display for PasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Length => "Password must contain 15–128 Unicode code points",
            Self::Compromised => "Choose a password absent from the compromised-password list",
            Self::BlocklistUnavailable => "Password blocklist unavailable",
            Self::Overloaded => "Password workers at capacity",
            Self::Unavailable => "Password processing unavailable",
            Self::InvalidHash => "Unsupported stored password hash",
        })
    }
}
impl std::error::Error for PasswordError {}

#[derive(Clone)]
pub struct PasswordDigest(String);
impl fmt::Debug for PasswordDigest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PasswordDigest([REDACTED])")
    }
}
impl PasswordDigest {
    /// Accept only this release's supported cost parameters, preventing corrupt
    /// stored PHC strings from allocating unbounded memory or CPU during verify.
    pub fn from_phc(value: String) -> Result<Self, PasswordError> {
        let parsed = PasswordHash::new(&value).map_err(|_| PasswordError::InvalidHash)?;
        let params = Params::try_from(&parsed).map_err(|_| PasswordError::InvalidHash)?;
        if parsed.algorithm.as_str() != "argon2id"
            || parsed.version != Some(19)
            || params.m_cost() != 19 * 1024
            || params.t_cost() != 2
            || params.p_cost() != 1
            || params.output_len().unwrap_or(32) != 32
            || parsed.salt.is_none()
            || parsed.hash.is_none()
        {
            return Err(PasswordError::InvalidHash);
        }
        Ok(Self(value))
    }
    pub fn as_phc(&self) -> &str {
        &self.0
    }
}

#[derive(Clone)]
pub struct PasswordService {
    blocked: Arc<HashSet<String>>,
    workers: Arc<Semaphore>,
    admission: Arc<Semaphore>,
}
impl PasswordService {
    /// Load a provisioned list at initialization, not on an async request worker.
    /// A missing, empty or non-UTF-8 list cannot construct a password service.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, PasswordError> {
        let text =
            std::fs::read_to_string(path).map_err(|_| PasswordError::BlocklistUnavailable)?;
        Self::from_text(&text)
    }
    fn from_text(text: &str) -> Result<Self, PasswordError> {
        let blocked: HashSet<_> = text
            .lines()
            .filter(|s| !s.is_empty())
            .map(str::to_lowercase)
            .collect();
        if blocked.is_empty() {
            return Err(PasswordError::BlocklistUnavailable);
        }
        Ok(Self {
            blocked: Arc::new(blocked),
            workers: Arc::new(Semaphore::new(WORKERS)),
            admission: Arc::new(Semaphore::new(WORKERS + QUEUED)),
        })
    }
    pub fn validate(&self, password: &str) -> Result<(), PasswordError> {
        if !(15..=128).contains(&password.chars().take(129).count()) {
            return Err(PasswordError::Length);
        }
        // Matching is case-insensitive, but hashing always uses original bytes.
        if self.blocked.contains(&password.to_lowercase()) {
            return Err(PasswordError::Compromised);
        }
        Ok(())
    }
    async fn run<T, F>(&self, job: F) -> Result<T, PasswordError>
    where
        T: Send + 'static,
        F: FnOnce() -> Result<T, PasswordError> + Send + 'static,
    {
        let admission = self
            .admission
            .clone()
            .try_acquire_owned()
            .map_err(|_| PasswordError::Overloaded)?;
        let worker = self
            .workers
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| PasswordError::Unavailable)?;
        tokio::task::spawn_blocking(move || {
            // Both permits stay with the blocking job if its caller disconnects.
            let (_admission, _worker) = (admission, worker);
            job()
        })
        .await
        .map_err(|_| PasswordError::Unavailable)?
    }
    pub async fn hash(&self, password: String) -> Result<PasswordDigest, PasswordError> {
        self.validate(&password)?;
        self.run(move || {
            engine()
                .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
                .map(|hash| PasswordDigest(hash.to_string()))
                .map_err(|_| PasswordError::Unavailable)
        })
        .await
    }
    /// Verify unchanged bytes; do not reject existing passwords because the list
    /// changed. Future login must use a same-cost dummy digest for unknown users.
    pub async fn verify(
        &self,
        password: String,
        digest: PasswordDigest,
    ) -> Result<bool, PasswordError> {
        if password.chars().take(129).count() > 128 {
            return Ok(false);
        }
        self.run(move || {
            let parsed =
                PasswordHash::new(digest.as_phc()).map_err(|_| PasswordError::InvalidHash)?;
            match engine().verify_password(password.as_bytes(), &parsed) {
                Ok(()) => Ok(true),
                Err(argon2::password_hash::Error::Password) => Ok(false),
                Err(_) => Err(PasswordError::InvalidHash),
            }
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn service() -> PasswordService {
        PasswordService::load("tests/fixtures/compromised-passwords.txt").unwrap()
    }
    #[test]
    fn policy_and_missing_list_fail_closed() {
        let p = service();
        assert_eq!(p.validate(&"x".repeat(14)), Err(PasswordError::Length));
        assert!(p.validate(&"🦀".repeat(15)).is_ok());
        assert!(p.validate(&"🦀".repeat(128)).is_ok());
        assert_eq!(p.validate(&"🦀".repeat(129)), Err(PasswordError::Length));
        assert_eq!(
            p.validate("PASSWORDPASSWORD"),
            Err(PasswordError::Compromised)
        );
        assert!(p.validate(" a unique spaced phrase 🦀 ").is_ok());
        assert!(matches!(
            PasswordService::load("tests/fixtures/missing-list"),
            Err(PasswordError::BlocklistUnavailable)
        ));
        assert!(matches!(
            PasswordService::from_text("\n"),
            Err(PasswordError::BlocklistUnavailable)
        ));
    }
    #[tokio::test]
    async fn original_bytes_salts_and_stored_costs() {
        let p = service();
        let value = "  Unique e\u{301} phrase 🦀  ";
        let a = p.hash(value.into()).await.unwrap();
        let b = p.hash(value.into()).await.unwrap();
        assert_ne!(a.as_phc(), b.as_phc());
        assert!(a.as_phc().starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(
            p.verify(
                value.into(),
                PasswordDigest::from_phc(a.as_phc().into()).unwrap()
            )
            .await
            .unwrap()
        );
        for wrong in [
            value.trim(),
            "  Unique é phrase 🦀  ",
            "  unique e\u{301} phrase 🦀  ",
        ] {
            assert!(!p.verify(wrong.into(), a.clone()).await.unwrap());
        }
        assert!(PasswordDigest::from_phc(a.as_phc().replace("m=19456", "m=999999999")).is_err());
        assert!(PasswordDigest::from_phc(a.as_phc().replace("argon2id", "argon2i")).is_err());
        assert!(PasswordDigest::from_phc("corrupt".into()).is_err());
        assert!(!format!("{a:?}").contains("argon2id"));
    }
    async fn until(condition: impl Fn() -> bool) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            while !condition() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("worker/admission state must settle within the test deadline");
    }
    #[tokio::test(flavor = "current_thread")]
    async fn workers_queue_cancellation_and_async_progress_are_bounded() {
        let p = service();
        let gate = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
        let started = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..WORKERS {
            let (p, gate, started) = (p.clone(), gate.clone(), started.clone());
            tasks.push(tokio::spawn(async move {
                p.run(move || {
                    started.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    let (lock, ready) = &*gate;
                    let _guard = ready
                        .wait_timeout_while(
                            lock.lock().unwrap(),
                            std::time::Duration::from_secs(10),
                            |open| !*open,
                        )
                        .unwrap();
                    Ok(())
                })
                .await
            }));
        }
        until(|| started.load(std::sync::atomic::Ordering::SeqCst) == WORKERS).await;
        assert_eq!(p.workers.available_permits(), 0);
        // Aborting a caller cannot free permits while its blocking job is alive.
        tasks[0].abort();
        tokio::task::yield_now().await;
        for _ in 0..QUEUED {
            let p = p.clone();
            tasks.push(tokio::spawn(async move { p.run(|| Ok(())).await }));
        }
        until(|| p.admission.available_permits() == 0).await;
        assert_eq!(p.run(|| Ok(())).await, Err(PasswordError::Overloaded));
        // Waiting callers are cancellable and return their admission slot.
        tasks.last().unwrap().abort();
        until(|| p.admission.available_permits() > 0).await;
        // Reaching here on a single-thread runtime proves async progress while
        // all blocking workers remain held behind the gate.
        {
            let (lock, ready) = &*gate;
            *lock.lock().unwrap() = true;
            ready.notify_all();
        }
        for task in tasks {
            let _ = task.await;
        }
        until(|| p.admission.available_permits() == WORKERS + QUEUED).await;
        assert_eq!(p.admission.available_permits(), WORKERS + QUEUED);
        assert_eq!(p.workers.available_permits(), WORKERS);
    }
}
