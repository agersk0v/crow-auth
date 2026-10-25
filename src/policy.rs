//! Canonicalizes and validates usernames and ASCII email addresses before storage.

use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputError {
    Username,
    Email,
}
impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Username => "Invalid username",
            Self::Email => "Invalid email",
        })
    }
}
impl std::error::Error for InputError {}

// Do not derive Debug: identifiers must not enter security logs accidentally.
pub struct Username(String);
impl Username {
    pub fn parse(input: &str) -> Result<Self, InputError> {
        let value = input.trim().to_ascii_lowercase();
        if !(3..=32).contains(&value.len())
            || !value
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
        {
            return Err(InputError::Username);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

pub struct Email(String);
impl Email {
    pub fn parse(input: &str) -> Result<Self, InputError> {
        let value = input.trim().to_ascii_lowercase();
        if !value.is_ascii() || value.len() > 254 {
            return Err(InputError::Email);
        }
        let Some((local, domain)) = value.split_once('@') else {
            return Err(InputError::Email);
        };
        let atom = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~".contains(&b);
        if !local
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(atom))
        {
            return Err(InputError::Email);
        }
        let labels: Vec<_> = domain.split('.').collect();
        if labels.len() < 2
            || !labels.iter().all(|label| {
                (1..=63).contains(&label.len())
                    && label.as_bytes()[0].is_ascii_alphanumeric()
                    && label.as_bytes()[label.len() - 1].is_ascii_alphanumeric()
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            })
        {
            return Err(InputError::Email);
        }
        Ok(Self(value))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn usernames_are_canonical_and_bounded() {
        assert_eq!(
            Username::parse(" \tUser_Name\n").unwrap().as_str(),
            "user_name"
        );
        for value in ["ab", "a-b", "üser", "a b", &"a".repeat(33)] {
            assert!(Username::parse(value).is_err());
        }
        assert!(Username::parse(&"a".repeat(32)).is_ok());
    }
    #[test]
    fn ascii_dot_atom_email_and_dns_labels() {
        assert_eq!(
            Email::parse("  A.B+Tag@EXAMPLE.COM \n").unwrap().as_str(),
            "a.b+tag@example.com"
        );
        for value in [
            "",
            "a@localhost",
            "a@@b.com",
            ".a@b.com",
            "a..b@b.com",
            "a.@b.com",
            "a@-b.com",
            "a@b-.com",
            "a@b..com",
            "a@b.com.",
            "a b@b.com",
            "\"a\"@b.com",
            "ü@b.com",
            "a@bücher.com",
            "a@[127.0.0.1]",
        ] {
            assert!(Email::parse(value).is_err(), "accepted invalid email");
        }
        assert!(Email::parse(&format!("a@{}.com", "b".repeat(64))).is_err());
        let max = format!(
            "{}@{}.{}.com",
            "a".repeat(124),
            "b".repeat(63),
            "c".repeat(61)
        );
        assert_eq!(max.len(), 254);
        assert!(Email::parse(&max).is_ok());
        assert!(Email::parse(&format!("x{max}")).is_err());
        // Provider-specific dots and plus aliases are not collapsed.
        assert_ne!(
            Email::parse("a.b@x.com").unwrap().as_str(),
            Email::parse("ab@x.com").unwrap().as_str()
        );
    }
}
