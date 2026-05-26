import sys
from typing import Any, MutableMapping

import toml


ConfigType = MutableMapping[str, Any]


class ConfigError(Exception):
    """Raised when the configuration file is missing, invalid, or incomplete."""


def _require_section(config: ConfigType, section: str, filename: str) -> ConfigType:
    value = config.get(section)
    if not isinstance(value, MutableMapping):
        raise ConfigError(
            f"Config '{filename}' is missing required section [{section}]."
        )
    return value


def _require_key(section: ConfigType, section_name: str, key: str, filename: str) -> Any:
    if key not in section:
        raise ConfigError(
            f"Config '{filename}' section [{section_name}] is missing required key '{key}'."
        )
    return section[key]


def _validate_config(config: ConfigType, filename: str) -> None:
    service = _require_section(config, "service", filename)
    network = _require_key(service, "service", "network", filename)
    if not isinstance(network, str) or not network:
        raise ConfigError(
            f"Config '{filename}' section [service] must set a non-empty 'network'."
        )

    _require_section(config, network, filename)
    _require_section(config, "web_interface", filename)
    _require_section(config, "utxo", filename)
    _require_section(config, "dynamic_config", filename)

    web_interface = config["web_interface"]
    for key in ("address", "log_level", "reload", "rust_url"):
        _require_key(web_interface, "web_interface", key, filename)


def load_config(filename: str) -> ConfigType:
    """Load and validate config from the provided TOML file."""
    try:
        with open(filename, "r") as f:
            config = toml.load(f)
    except FileNotFoundError as e:
        raise ConfigError(
            f"Unable to read config file '{filename}'. "
            "Check the path and that the file exists."
        ) from e
    except toml.TomlDecodeError as e:
        raise ConfigError(
            f"Unable to parse config file '{filename}': {e}"
        ) from e

    if not isinstance(config, MutableMapping):
        raise ConfigError(f"Config '{filename}' must contain a TOML table at the top level.")

    _validate_config(config, filename)
    return config


def load_config_or_exit(filename: str) -> ConfigType:
    """Load config, printing a clear error and exiting on failure."""
    try:
        return load_config(filename)
    except ConfigError as e:
        print(f"Error: {e}", file=sys.stderr)
        sys.exit(1)
