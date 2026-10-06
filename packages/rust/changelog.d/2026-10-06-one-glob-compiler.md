**Changed** A declared `[[table]] glob` is compiled by the same glob compiler
as a path-table, so brace ranges such as `f{1..3}.md` and the other bash brace
rules now read the same on both surfaces.
