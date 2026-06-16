"""Framework adapters. Each depends only on the Taro SDK core, never the reverse.

    from taro.integrations.ultralytics import attach
"""

from .ultralytics import attach

__all__ = ["attach"]
