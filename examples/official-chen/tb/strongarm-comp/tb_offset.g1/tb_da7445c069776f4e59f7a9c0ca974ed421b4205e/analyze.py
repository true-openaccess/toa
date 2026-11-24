"""The offset: the input at which the decisions turn over."""


def offset(vin, decisions):
    for v, d in zip(vin, decisions):
        if d:
            return v * 1e3
    return None
