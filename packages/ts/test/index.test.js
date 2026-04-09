const test = require("node:test");
const assert = require("node:assert/strict");

const sdk = require("../index.js");

test("placeholder export reports status", () => {
  assert.equal(
    sdk.message,
    "dirsql TypeScript SDK is not yet implemented. See https://github.com/thekevinscott/dirsql for status."
  );
});
