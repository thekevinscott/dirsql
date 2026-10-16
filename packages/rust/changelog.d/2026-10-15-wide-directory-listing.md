**Changed**

- **Wide directories list faster.** A directory of hundreds of thousands of entries, such as one listed by `./*`, makes its rows on all cores, and on Linux its entries are read without a copy per name. Output is unchanged.
