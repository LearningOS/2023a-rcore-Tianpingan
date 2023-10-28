//! Implementation of [`PageTableEntry`] and [`PageTable`].

use super::{frame_alloc, FrameTracker, PhysPageNum, StepByOne, VirtAddr, VirtPageNum};
use alloc::vec;
use alloc::vec::Vec;
use bitflags::*;

bitflags! {
    /// page table entry flags
    pub struct PTEFlags: u8 {
        const V = 1 << 0;
        const R = 1 << 1;
        const W = 1 << 2;
        const X = 1 << 3;
        const U = 1 << 4;
        const G = 1 << 5;
        const A = 1 << 6;
        const D = 1 << 7;
    }
}

#[derive(Copy, Clone)]
#[repr(C)]
/// page table entry structure
pub struct PageTableEntry {
    /// bits of page table entry
    pub bits: usize,
}

impl PageTableEntry {
    /// Create a new page table entry
    pub fn new(ppn: PhysPageNum, flags: PTEFlags) -> Self {
        PageTableEntry {
            bits: ppn.0 << 10 | flags.bits as usize,
        }
    }
    /// Create an empty page table entry
    pub fn empty() -> Self {
        PageTableEntry { bits: 0 }
    }
    /// Get the physical page number from the page table entry
    pub fn ppn(&self) -> PhysPageNum {
        (self.bits >> 10 & ((1usize << 44) - 1)).into()
    }
    /// Get the flags from the page table entry
    pub fn flags(&self) -> PTEFlags {
        PTEFlags::from_bits(self.bits as u8).unwrap()
    }
    /// The page pointered by page table entry is valid?
    pub fn is_valid(&self) -> bool {
        (self.flags() & PTEFlags::V) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is readable?
    pub fn readable(&self) -> bool {
        (self.flags() & PTEFlags::R) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is writable?
    pub fn writable(&self) -> bool {
        (self.flags() & PTEFlags::W) != PTEFlags::empty()
    }
    /// The page pointered by page table entry is executable?
    pub fn executable(&self) -> bool {
        (self.flags() & PTEFlags::X) != PTEFlags::empty()
    }
}

/// page table structure
pub struct PageTable {
    root_ppn: PhysPageNum,
    frames: Vec<FrameTracker>,
}

/// Assume that it won't oom when creating/mapping.
impl PageTable {
    /// Create a new page table
    pub fn new() -> Self {
        let frame = frame_alloc().unwrap();
        PageTable {
            root_ppn: frame.ppn,
            frames: vec![frame],
        }
    }
    /// Temporarily used to get arguments from user space.
    pub fn from_token(satp: usize) -> Self {
        Self {
            root_ppn: PhysPageNum::from(satp & ((1usize << 44) - 1)),
            frames: Vec::new(),
        }
    }
    /// Find PageTableEntry by VirtPageNum, create a frame for a 4KB page table if not exist
    fn find_pte_create(&mut self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                let frame = frame_alloc().unwrap();
                *pte = PageTableEntry::new(frame.ppn, PTEFlags::V);
                self.frames.push(frame);
            }
            ppn = pte.ppn();
        }
        result
    }

    /// Find PageTableEntry by VirtPageNum
    fn find_pte(&self, vpn: VirtPageNum) -> Option<&mut PageTableEntry> {
        let idxs = vpn.indexes();
        let mut ppn = self.root_ppn;
        let mut result: Option<&mut PageTableEntry> = None;
        for (i, idx) in idxs.iter().enumerate() {
            let pte = &mut ppn.get_pte_array()[*idx];
            if i == 2 {
                result = Some(pte);
                break;
            }
            if !pte.is_valid() {
                return None;
            }
            ppn = pte.ppn();
        }
        result
    }
    /// set the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn map(&mut self, vpn: VirtPageNum, ppn: PhysPageNum, flags: PTEFlags) {
        // trace!(
        //     "mmap-inner-inner: vpn: {}, ppn: {}, flags: {}",
        //     vpn.0,
        //     ppn.0,
        //     flags.bits
        // );

        let pte = self.find_pte_create(vpn).unwrap();
        assert!(!pte.is_valid(), "vpn {:?} is mapped before mapping", vpn);
        *pte = PageTableEntry::new(ppn, flags | PTEFlags::V);
    }
    /// remove the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn unmap(&mut self, vpn: VirtPageNum) {
        let pte = self.find_pte(vpn).unwrap();
        assert!(pte.is_valid(), "vpn {:?} is invalid before unmapping", vpn);
        *pte = PageTableEntry::empty();
    }
    /// get the page table entry from the virtual page number
    pub fn translate(&self, vpn: VirtPageNum) -> Option<PageTableEntry> {
        self.find_pte(vpn).map(|pte| *pte)
    }
    /// get the token from the page table
    pub fn token(&self) -> usize {
        8usize << 60 | self.root_ppn.0
    }
    /// set the map between virtual page number and physical page number
    #[allow(unused)]
    pub fn mmap(&mut self, vpn: VirtPageNum, flags: PTEFlags) -> Option<()> {
        // 首先看有没有已经映射了的
        trace!("mmap-inner: vpn: {}, flags: {}", vpn.0, flags.bits);
        let frame = frame_alloc()?;
        self.map(vpn, frame.ppn, flags);
        self.frames.push(frame);
        Some(())
    }
}

/// Translate&Copy a ptr[u8] array with LENGTH len to a mutable u8 Vec through page table
pub fn translated_byte_buffer(token: usize, ptr: *const u8, len: usize) -> Vec<&'static mut [u8]> {
    let page_table = PageTable::from_token(token);
    let mut start = ptr as usize;
    let end = start + len;
    // trace!("start: {}, end: {}", start, end);
    let mut v = Vec::new();
    while start < end {
        let start_va = VirtAddr::from(start);
        let mut vpn = start_va.floor();
        let ppn = page_table.translate(vpn).unwrap().ppn();
        vpn.step();
        let mut end_va: VirtAddr = vpn.into();
        end_va = end_va.min(VirtAddr::from(end));
        if end_va.page_offset() == 0 {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..]);
        } else {
            v.push(&mut ppn.get_bytes_array()[start_va.page_offset()..end_va.page_offset()]);
        }
        start = end_va.into();
    }
    v
}

// /// for lab4
// /// start和len都需要pagesize对齐
// /// port是后8位置
// pub fn mmap(token: usize, start: usize, len: usize, port: u8) -> isize {
//     trace!("mmap[{}]: start: {}, len: {}, port: {}", token, start, len, port);
//     let mut flags = PTEFlags { bits: port << 1 };
//     flags.set(PTEFlags::U, true);
//     // trace!("bits: {}", flags.bits());
//     let mut page_table = PageTable::from_token(token);

//     let res = (start .. start + len).step_by(PAGE_SIZE).all(|x| {
//         trace!("x = {}", x);
//         let start_va = VirtAddr::from(x);
//         let vpn = start_va.floor();
//         trace!("mmap vpn = {}", vpn.0);
//         page_table.translate(vpn).is_none() || !page_table.translate(vpn).unwrap().is_valid()
//     });
//     if !res {
//         return -1;
//     }
//     (start .. start + len).step_by(PAGE_SIZE).for_each(|x| {
//         trace!("xx = {}", x);
//         let start_va = VirtAddr::from(x);
//         let vpn = start_va.floor();
//         if page_table.mmap(vpn, flags).is_none() {
//             trace!("fuck you");
//         }
//     });
//     0
// }
// /// for lab4
// pub fn unmmap(token: usize, start: usize, len: usize) -> isize {
//     trace!("unmmap[{}]: start: {}, len: {}", token, start, len);
//     let mut page_table = PageTable::from_token(token);
//     let res = (start .. start + len).step_by(PAGE_SIZE).all(|x| {
//         trace!("unmmap x = {}", x);
//         let start_va = VirtAddr::from(x);
//         let vpn = start_va.floor();
//         trace!("unmmap vpn = {}", vpn.0);
//         page_table.translate(vpn).is_some() && page_table.translate(vpn).unwrap().is_valid()
//     });
//     trace!("unmmap: res = {}", res);
//     if !res {
//         return -1;
//     }
//     (start .. start + len).step_by(PAGE_SIZE).for_each(|x| {
//         let start_va = VirtAddr::from(x);
//         let vpn = start_va.floor();
//         page_table.unmap(vpn);
//     });
//     0
// }
