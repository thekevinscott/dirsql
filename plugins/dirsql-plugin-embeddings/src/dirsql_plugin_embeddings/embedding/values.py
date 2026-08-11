import base64
import binascii


class ProtocolError(ValueError):
    """A request value that does not follow the worker protocol."""


def decode_value(value):
    if value is None:
        return None
    if isinstance(value, str):
        return value
    if isinstance(value, dict) and set(value) == {"$bytes"}:
        encoded = value["$bytes"]
        if not isinstance(encoded, str):
            raise ProtocolError('the "$bytes" value must be a base64 string')
        try:
            raw = base64.b64decode(encoded, validate=True)
        except binascii.Error as error:
            raise ProtocolError(f'invalid base64 in "$bytes": {error}') from error
        try:
            return raw.decode("utf-8")
        except UnicodeDecodeError as error:
            raise ProtocolError(f"BLOB is not valid utf-8 text: {error}") from error
    raise ProtocolError(
        f"embed() accepts TEXT or BLOB values, got {type(value).__name__}"
    )
