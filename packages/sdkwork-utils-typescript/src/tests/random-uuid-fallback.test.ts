import assert from "node:assert/strict";
import test from "node:test";

import { uuid } from "../id.js";
import { randomUuid } from "../runtime/random.js";

const UUID_V4_PATTERN =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

test("uuid falls back when crypto.randomUUID is missing", () => {
  const original = globalThis.crypto;
  const values = new Uint8Array(16).fill(7);
  Object.defineProperty(globalThis, "crypto", {
    configurable: true,
    value: {
      getRandomValues(target: Uint8Array) {
        target.set(values.subarray(0, target.length));
        return target;
      },
    },
  });

  try {
    assert.match(uuid(), UUID_V4_PATTERN);
    assert.match(randomUuid(), UUID_V4_PATTERN);
  } finally {
    Object.defineProperty(globalThis, "crypto", {
      configurable: true,
      value: original,
    });
  }
});

test("uuid falls back when crypto.randomUUID is not a function", () => {
  const original = globalThis.crypto;
  Object.defineProperty(globalThis, "crypto", {
    configurable: true,
    value: {
      getRandomValues(target: Uint8Array) {
        for (let index = 0; index < target.length; index += 1) {
          target[index] = (index * 17) & 0xff;
        }
        return target;
      },
      randomUUID: true,
    },
  });

  try {
    assert.match(uuid(), UUID_V4_PATTERN);
  } finally {
    Object.defineProperty(globalThis, "crypto", {
      configurable: true,
      value: original,
    });
  }
});
