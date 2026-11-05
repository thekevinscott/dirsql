**Fixed** A watch refresh that moves a `UNIQUE` value between keyed rows (a
swap, a rotation, a renumbered `position UNIQUE`) now applies, with an
`update` event per row, as a fresh build of the same files would. It used to
fail with `UNIQUE constraint failed` and keep the old rows. A row whose
`UPDATE` collides with a value another row is giving up is deleted and
inserted again at its rowid.
