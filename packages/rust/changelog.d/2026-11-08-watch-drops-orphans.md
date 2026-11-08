**Changed** In watch, a batch whose rows break a foreign key no longer fails
whole and waits for a later batch to resolve it. The rows of each file holding
an offending row are left out, one error naming that file is reported, and the
rest of the batch lands. A per-table hook whose new rows break a foreign key
keeps its previous rows, less any a deleted parent strands, and reports one
error naming its table. A left-out
file returns when it is next read: its own file event, a scan of a directory
renamed in, or a rescan. A parent arriving alone does not bring it back.
