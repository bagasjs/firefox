/*
 * Any copyright is dedicated to the Public Domain.
 * http://creativecommons.org/licenses/publicdomain/
 */

/*---
includes: [sm/non262.js, sm/non262-shell.js]
flags:
  - noStrict
description: |
  pending
esid: pending
---*/
function C(){}
C.prototype = 1;
assert.sameValue(Object.getOwnPropertyDescriptor(C, "prototype").configurable, false);


reportCompare(0, 0);
