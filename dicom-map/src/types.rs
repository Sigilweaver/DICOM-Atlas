use std::fmt;

/// `Vm::max` value meaning "unbounded" (`n`).
pub const VM_UNBOUNDED: u16 = 0xFFFF;

/// Value multiplicity of a tag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct Vm {
    /// Lower bound.
    pub min: u16,
    /// Upper bound, or [`VM_UNBOUNDED`].
    pub max: u16,
    /// For `k-kn` the multiple `k`; `1` otherwise.
    pub step: u16,
}

impl Vm {
    /// The standard declares no VM for this (retired) tag.
    pub const UNDECLARED: Vm = Vm {
        min: 0,
        max: 0,
        step: 1,
    };

    pub const fn new(min: u16, max: u16, step: u16) -> Vm {
        Vm { min, max, step }
    }

    /// Upper bound, `None` when unbounded.
    pub fn max_bound(&self) -> Option<u16> {
        (self.max != VM_UNBOUNDED).then_some(self.max)
    }
}

impl fmt::Display for Vm {
    /// `k`, `k-m`, `k-n`, `k-kn`, or the empty string when undeclared.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if *self == Vm::UNDECLARED {
            Ok(())
        } else if self.max == VM_UNBOUNDED && self.step > 1 {
            write!(f, "{}-{}n", self.min, self.step)
        } else if self.max == VM_UNBOUNDED {
            write!(f, "{}-n", self.min)
        } else if self.min == self.max {
            write!(f, "{}", self.min)
        } else {
            write!(f, "{}-{}", self.min, self.max)
        }
    }
}

/// Per-record flag bits. Bits this version does not define are preserved and
/// should be ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TagFlags(u32);

impl TagFlags {
    /// The tag is retired in the standard.
    pub const RETIRED: TagFlags = TagFlags(1 << 0);
    /// `element` is a private block offset (the low byte of the element).
    pub const BLOCK_OFFSET: TagFlags = TagFlags(1 << 1);

    pub const fn from_bits(bits: u32) -> TagFlags {
        TagFlags(bits)
    }

    pub const fn bits(self) -> u32 {
        self.0
    }

    pub const fn contains(self, other: TagFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn union(self, other: TagFlags) -> TagFlags {
        TagFlags(self.0 | other.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_display() {
        assert_eq!(Vm::new(1, 1, 1).to_string(), "1");
        assert_eq!(Vm::new(1, 3, 1).to_string(), "1-3");
        assert_eq!(Vm::new(1, VM_UNBOUNDED, 1).to_string(), "1-n");
        assert_eq!(Vm::new(2, VM_UNBOUNDED, 2).to_string(), "2-2n");
        assert_eq!(Vm::UNDECLARED.to_string(), "");
    }
}
