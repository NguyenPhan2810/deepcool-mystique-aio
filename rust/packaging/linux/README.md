# Linux packaging moved

The udev rule, the daemon's systemd user unit and the install notes now live in the
repository-root `packaging/linux/` directory, next to the Windows packaging. One device,
one rule set: installing two rules for `3633:0009` from two directories is how the
wrong `MODE` wins.

`mystique.service` in this directory is the unit for the diagnostic CLI (`mystique
live`). It is not enabled by default and must not run while `mystique-daemon` owns the
panel.
