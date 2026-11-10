# Credits and licences for the film simulation data

Develop's film stocks (DV-4) are simulated by **spektrafilm**, created by Andrea Volpato
(<https://github.com/andreavolpato/spektrafilm>), through **spektrafilm-rs**, its Rust port by
turbasvin (<https://github.com/turbasvin/spektrafilm-rs>), used unmodified at commit
`9dd59b0380194b93686aaa230a8bb9680aa270a4` under the GNU General Public License v3.0.

The files in this directory are copied unmodified from spektrafilm-rs's `data/` at that commit:

- `profiles/` — film and paper profiles digitised from the manufacturers' datasheets by Andrea
  Volpato, licensed under **Creative Commons Attribution-ShareAlike 4.0 International (CC BY-SA
  4.0)**. The licence's text and preamble are in `SPEKTRAFILM_LICENSE.txt`, copied unmodified from
  spektrafilm at commit `3bb2c2d2801ff68b92019cf1dbcbb133d60832bc`.
- `luts/spectral_upsampling/irradiance_xy_tc.npy` and `filters/neutral_print_filters.json` — the
  spectral upsampling table and the neutral print filter database the engine reads.

Film simulation powered by spektrafilm.

Kodak, Portra, Ektar, Gold, Ultramax, Ektachrome, Kodachrome, Vision3, Verita, Fujifilm, Provia,
Velvia and the other product names are trademarks of their owners. They are used only to say which
film stock is simulated; neither project nor this one is affiliated with or endorsed by them.
