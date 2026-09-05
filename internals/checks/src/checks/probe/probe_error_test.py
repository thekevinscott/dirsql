import pytest

from checks.probe.probe_error import ProbeError


def describe_probe_error():
    def it_is_a_runtime_error():
        assert issubclass(ProbeError, RuntimeError)

    def it_carries_the_diagnostic():
        with pytest.raises(ProbeError, match="static-pie cannot dlopen"):
            raise ProbeError("static-pie cannot dlopen")
