**Changed**

- **Wide directories sort while they are read.** A directory of more than 8192 entries has each batch of entries sorted as the next is read, and the batches merged on all cores, so little of the sort is left once the read ends. Output is unchanged.
