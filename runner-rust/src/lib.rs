// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The parts of the runner that are worth testing from outside it: the codecs,
//! the shared JSON escaper, and the timing reduction. The binary in `main.rs`
//! is the orchestration around them.

pub mod codecs;
pub mod json;
pub mod timing;
