**Added**

- **The worker now tells dirsql which embeddings cost nothing.** Every `ok` response carries `"meta": {"cached": <bool>}`, so the progress line can split its worker-call count — `dirsql: ran 41231 worker calls in 2m41s (38104 cached)` — instead of reporting one undifferentiated total for a run that was mostly disk reads. `cachetta`'s wrapper returns a hit and a miss identically, so the flag is recovered from whether the wrapped compute actually ran. (#1034)
