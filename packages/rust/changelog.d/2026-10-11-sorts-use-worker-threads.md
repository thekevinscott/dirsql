**Changed**

- **A `GROUP BY` or `ORDER BY` over a large table now sorts on background threads.** A grouped query over a million parsed rows runs about 9% faster. Output is unchanged.
