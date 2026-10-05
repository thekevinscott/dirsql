**Changed** Path-table scans of large directories are faster. Names are
sorted by a cheaper key, and the entries of a directory holding thousands of
files are judged on every core. A `./*` query over a 1M-file directory drops
from 1.65s to 1.05s. The files returned and their order are unchanged.
