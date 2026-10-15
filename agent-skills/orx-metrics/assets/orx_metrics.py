"""Report metrics to orx: each call prints one `ORX_METRIC {json}` line.

Save this file next to the training code, then:

    from orx_metrics import log_metric, log_metrics

    log_metric("val_ic", ic, step=step)
    log_metrics({"train_loss": loss, "lr": lr}, step=step, split="train")

orx reads these lines from the run's output, so nothing else is needed. In
distributed training, call it from rank 0 only. NaN and infinities are
written as-is; orx records them as missing values and flags the run.
"""

import json
import sys

_RESERVED = ("step", "split", "key", "value")


def log_metric(key, value, step=None, split=None, file=None):
    """Report one value of one metric."""
    record = {"key": str(key), "value": _number(value)}
    _add_context(record, step, split)
    _emit(record, file)


def log_metrics(values, step=None, split=None, file=None):
    """Report several metrics measured at the same step."""
    record = {}
    _add_context(record, step, split)
    for key, value in values.items():
        if key in _RESERVED:
            raise ValueError(f"{key!r} is reserved; pass it as an argument instead")
        record[str(key)] = _number(value)
    _emit(record, file)


def _add_context(record, step, split):
    if step is not None:
        record["step"] = int(step)
    if split is not None:
        record["split"] = str(split)


def _number(value):
    # NumPy and PyTorch scalars, and one-element tensors.
    if hasattr(value, "item"):
        value = value.item()
    return float(value)


def _emit(record, file):
    print("ORX_METRIC " + json.dumps(record), file=file or sys.stdout, flush=True)
