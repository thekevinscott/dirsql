**Fixed**

- **`embed()` no longer renders a progress bar for every call.** The worker embeds exactly one value per protocol round trip, so the per-call tqdm bar could only ever say `1/1` — on a TTY a 20-file corpus query printed ~21 of them to stderr. The bar is gone; the Hugging Face model-*download* bar, which measures something worth watching, is unchanged and still TTY-gated. (#814)
