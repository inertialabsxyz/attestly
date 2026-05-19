# awp is a PEP 420 namespace package shared with `awp-core-py`. Do not put
# any code here — placing module-level imports here would shadow the
# C-extension `awp` module shipped by `awp-core-py`.
__path__ = __import__("pkgutil").extend_path(__path__, __name__)
