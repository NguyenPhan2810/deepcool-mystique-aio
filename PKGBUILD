# Maintainer: Nguyen Phan <nguyen@nguyenphan.org>
pkgname=deepcool-mystique-aio
pkgver=0.1.1
pkgrel=1
pkgdesc="Driver and control panel for the DeepCool MYSTIQUE AIO LCD"
arch=('x86_64')
url="https://github.com/NguyenPhan2810/deepcool-mystique-aio"
license=('MIT' 'Apache-2.0')
depends=('gtk3' 'webkit2gtk-4.1' 'hicolor-icon-theme' 'xdotool')
makedepends=('cargo' 'rust' 'pkgconf' 'libayatana-appindicator')
install="$pkgname.install"
# Vendored libusb is C; makepkg's LTO flags break the link step.
options=(!lto)
source=("$pkgname-$pkgver.tar.gz::$url/archive/v$pkgver.tar.gz")
sha256sums=('967917fee323e48326cf8a75d8d45abbcf82cdbf4ededd85e64813e7e5525e78')

build() {
  cd "$srcdir/$pkgname-$pkgver/rust"
  cargo build --release --workspace --locked
}

package() {
  cd "$srcdir/$pkgname-$pkgver"

  install -Dm755 rust/target/release/mystique        "$pkgdir/usr/bin/mystique"
  install -Dm755 rust/target/release/mystique-daemon "$pkgdir/usr/bin/mystique-daemon"
  install -Dm755 rust/target/release/mystique-ui     "$pkgdir/usr/bin/mystique-ui"

  # Per-user unit; the shipped copy targets a ~/.local/bin install, a package does not.
  install -Dm644 packaging/linux/mystique-daemon.service \
    "$pkgdir/usr/lib/systemd/user/mystique-daemon.service"
  sed -i 's|%h/\.local/bin/mystique-daemon|/usr/bin/mystique-daemon|' \
    "$pkgdir/usr/lib/systemd/user/mystique-daemon.service"

  # Hosts that need an explicit ACL override this from /etc/udev/rules.d/.
  install -Dm644 packaging/linux/99-mystique.rules \
    "$pkgdir/usr/lib/udev/rules.d/99-mystique.rules"

  # The entry is named after the Wayland app id KWin reports, not the bundle identifier.
  install -Dm644 packaging/arch/mystique-ui.desktop \
    "$pkgdir/usr/share/applications/mystique-ui.desktop"

  for size in 16 24 32 48 64 128 256 512; do
    install -Dm644 "rust/apps/mystique-ui/icons/icon-${size}.png" \
      "$pkgdir/usr/share/icons/hicolor/${size}x${size}/apps/mystique.png"
  done

  install -Dm644 LICENSE-MIT    "$pkgdir/usr/share/licenses/$pkgname/LICENSE-MIT"
  install -Dm644 LICENSE-APACHE "$pkgdir/usr/share/licenses/$pkgname/LICENSE-APACHE"
}
