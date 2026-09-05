PATH_PREFIXES = ("./", "../", "/", "~/")


def normalize_glob(glob):
    # The core only rescues path-shaped missing tables (./, ../, /, ~/); a
    # bare relative glob like '**/*.md' would error with a "did you mean
    # './...'" hint. Here GLOB is unambiguously a corpus glob, so spare the
    # user the round trip.
    if glob.startswith(PATH_PREFIXES):
        return glob
    return f"./{glob}"
