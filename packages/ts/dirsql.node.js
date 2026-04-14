// Stub entry point used when the real napi-rs native addon (`dirsql.node`,
// a compiled binary) hasn't been built. Binding tests import this module
// and replace `module.exports.DirSQL` BEFORE importing the SDK.
//
// The real build produces `dirsql.node` (no `.js` extension) which Node
// loads in preference to this file.
module.exports = {
  DirSQL: class StubDirSQL {
    constructor() {
      throw new Error(
        "dirsql native addon is not built. Run `pnpm build` to produce " +
          "`dirsql.node`, or replace `module.exports.DirSQL` in tests " +
          "before importing the SDK.",
      );
    }
  },
};
