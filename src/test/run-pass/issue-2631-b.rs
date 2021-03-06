// Copyright 2012 The Rust Project Developers. See the COPYRIGHT
// file at the top-level directory of this distribution and at
// http://rust-lang.org/COPYRIGHT.
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

// xfail-fast
// aux-build:issue-2631-a.rs

extern mod req;

use req::request;
use std::hashmap::HashMap;

pub fn main() {
  let v = ~[@~"hi"];
  let mut m: req::header_map = HashMap::new();
  m.insert(~"METHOD", @mut v);
  request::<int>(&m);
}
