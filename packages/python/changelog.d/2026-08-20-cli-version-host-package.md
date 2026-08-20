**Fixed** — `dirsql --version` reports the installed wheel's version instead of
a frozen `0.2.7`: it read the embedded core crate's literal, which only the
crates.io release lane rewrites, so `uvx dirsql@0.4.20 --version` answered
`dirsql 0.2.7` (#958).
