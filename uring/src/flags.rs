// Having flags in terms of bitshifts is more readable.
#![allow(clippy::identity_op)]

use crate::raw::*;

use bitflags::bitflags;

bitflags! {
    #[derive(Default)]
    pub struct SetupFlags : u32 {
        const IOPOLL = IORING_SETUP_IOPOLL;
        const SQPOLL = IORING_SETUP_SQPOLL;
        const SQ_AFF = IORING_SETUP_SQ_AFF;
        const CQSIZE = IORING_SETUP_CQSIZE;
    }
}

bitflags! {
    #[derive(Default)]
    pub struct Features : u32 {
        const SINGLE_MMAP     = IORING_FEAT_SINGLE_MMAP;
        const NODROP          = IORING_FEAT_NODROP;
        const SUBMIT_STABLE   = IORING_FEAT_SUBMIT_STABLE;
        const RW_CUR_POS      = IORING_FEAT_RW_CUR_POS;
        const CUR_PERSONALITY = IORING_FEAT_CUR_PERSONALITY;
    }
}

bitflags! {
    #[derive(Default)]
    pub struct SubmissionFlags : u8 {
        const FIXED_FILE    = IOSQE_FIXED_FILE;
        const IO_DRAIN      = IOSQE_IO_DRAIN;
        const IO_LINK       = IOSQE_IO_LINK;
        const IO_HARDLINK   = IOSQE_IO_HARDLINK;
        const ASYNC         = IOSQE_ASYNC;
        const BUFFER_SELECT = IOSQE_BUFFER_SELECT;
    }
}

bitflags! {
    #[derive(Default)]
    pub struct CompletionFlags : u32 {
        // If set, the upper 16 bits are the buffer ID
        const BUFFER = 1 << 0;

        const BUFFER_ID_MASK = 0xFFFF << 16;
    }
}

bitflags! {
    #[derive(Default)]
    pub struct ProbeOpFlags: u16 {
        const SUPPORTED = 1 << 0;
    }
}

bitflags! {
    #[derive(Default)]
    pub struct TimeoutFlags : u32 {
        const ABS = 1 << 0;
    }
}

bitflags! {
    #[derive(Default)]
    pub struct EnterFlags : u32 {
        const GETEVENTS = 1 << 0;
        const SQ_WAKEUP = 1 << 1;
    }
}

impl CompletionFlags {
  pub fn buf_id(&self) -> Option<u16> {
    match self.contains(Self::BUFFER) {
      true => Some((self.bits() >> 16) as u16),
      false => None,
    }
  }
}
