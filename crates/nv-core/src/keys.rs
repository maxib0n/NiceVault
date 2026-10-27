use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroize;

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::fill(&mut b).expect("OS RNG unavailable");
    b
}

pub struct Key32(Box<[u8; 32]>);

impl Key32 {
    fn new_boxed() -> Self {
        let mut b = Box::new([0u8; 32]);
        unsafe {
            memsec::mlock(b.as_mut_ptr(), 32);
        }
        Key32(b)
    }
    pub fn random() -> Self {
        let mut k = Self::new_boxed();
        getrandom::fill(&mut k.0[..]).expect("OS RNG unavailable");
        k
    }
    pub fn from_slice(s: &[u8]) -> Self {
        let mut k = Self::new_boxed();
        k.0.copy_from_slice(&s[..32]);
        k
    }
    pub fn bytes(&self) -> &[u8; 32] {
        &self.0
    }
    pub fn derive(&self, info: &[u8]) -> Key32 {
        let mut k = Self::new_boxed();
        Hkdf::<Sha256>::new(None, &self.0[..]).expand(info, &mut k.0[..]).expect("hkdf len");
        k
    }
}

impl Clone for Key32 {
    fn clone(&self) -> Self {
        Self::from_slice(&self.0[..])
    }
}

impl Drop for Key32 {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

impl std::fmt::Debug for Key32 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Key32(<redacted>)")
    }
}
