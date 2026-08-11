import base64

import pytest

from . import values


def describe_decode_value():
    def it_passes_null_through():
        assert values.decode_value(None) is None

    def it_returns_text_unchanged():
        assert values.decode_value("hello world") == "hello world"

    def it_returns_the_empty_string_unchanged():
        assert values.decode_value("") == ""

    def it_decodes_tagged_bytes_as_utf8_text():
        encoded = base64.b64encode("héllo".encode("utf-8")).decode("ascii")
        assert values.decode_value({"$bytes": encoded}) == "héllo"

    def it_rejects_a_non_string_bytes_payload():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value({"$bytes": 7})
        assert '"$bytes" value must be a base64 string' in str(excinfo.value)

    def it_rejects_invalid_base64():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value({"$bytes": "!!!not-base64!!!"})
        assert 'invalid base64 in "$bytes"' in str(excinfo.value)

    def it_rejects_bytes_that_are_not_utf8():
        encoded = base64.b64encode(b"\xff\xfe").decode("ascii")
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value({"$bytes": encoded})
        assert "BLOB is not valid utf-8 text" in str(excinfo.value)

    def it_rejects_a_dict_with_extra_keys():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value({"$bytes": "aGk=", "extra": 1})
        assert "accepts TEXT or BLOB values, got dict" in str(excinfo.value)

    def it_rejects_a_dict_without_the_bytes_tag():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value({"other": "aGk="})
        assert "accepts TEXT or BLOB values, got dict" in str(excinfo.value)

    def it_rejects_an_empty_dict():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value({})
        assert "accepts TEXT or BLOB values, got dict" in str(excinfo.value)

    def it_rejects_base64_with_non_alphabet_characters_strictly():
        # "aG!k=" would decode as b"hi" if validation silently discarded the
        # "!"; strict validation must reject it instead.
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value({"$bytes": "aG!k="})
        assert 'invalid base64 in "$bytes"' in str(excinfo.value)

    def it_rejects_an_integer():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value(7)
        assert "accepts TEXT or BLOB values, got int" in str(excinfo.value)

    def it_rejects_a_float():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value(2.5)
        assert "accepts TEXT or BLOB values, got float" in str(excinfo.value)

    def it_rejects_a_list():
        with pytest.raises(values.ProtocolError) as excinfo:
            values.decode_value(["hello"])
        assert "accepts TEXT or BLOB values, got list" in str(excinfo.value)

    def it_is_a_value_error():
        assert issubclass(values.ProtocolError, ValueError)
