# CDC Badge Demo

Demo built using [Ariel OS][ariel-os-website] for the [CDC Badge hardware][cdc-badge-repo]: [datasheets list][cdc-badge-datasheets], [firmware checklist][cdc-badge-checklist], [schematics][cdc-badge-schematics].

Requires the Ariel OS toolchain to be installed, follow the [Ariel OS Getting Started][ariel-os-getting-started].

## Current features

- Initializes the critical hardware (power management IC for charging)
- Refreshes the screen and draws the Ariel OS logo

## Running

```sh
laze build run
```

## Feature ideas

- Add support for the keypad (IO expander)
- Add an UI using [mousefood](https://github.com/ratatui/mousefood)
- Interactive BLE scanner / advertiser
- Fetch data from Internet (wifi connectivity)
- Interact with the TROPIC01 secure element
- Upstream board support

[ariel-os-getting-started]: https://ariel-os.github.io/ariel-os/dev/docs/book/getting-started.html
[ariel-os-website]: https://ariel-os.org/
[cdc-badge-repo]: https://github.com/riatlabs/cdc-badge
[cdc-badge-datasheets]: https://github.com/riatlabs/cdc-badge/blob/main/docs/datasheets.md
[cdc-badge-checklist]: https://github.com/riatlabs/cdc-badge/blob/main/docs/firmware-checklist.md
[cdc-badge-schematics]: https://github.com/riatlabs/cdc-badge/releases/download/v1.1/cdc-badge_v1.1_schematic.pdf
