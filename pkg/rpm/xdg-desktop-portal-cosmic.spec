%global app_name xdg-desktop-portal-cosmic
%global appid org.freedesktop.impl.portal.desktop.cosmic
%global debug_package %{nil}

# _userunitdir comes from systemd-rpm-macros, which only exists on the Fedora
# host. The builder image is Debian-based, where rpmbuild leaves it unexpanded
# and fails with 'File must begin with "/"'. Define a fallback so the same spec
# builds in both places.
%{!?_userunitdir: %global _userunitdir /usr/lib/systemd/user}

Name:           %{app_name}
Epoch:          1
Version: 0.1.0
Release:        1%{?dist}
Summary:        XDG Desktop Portal backend for COSMIC (Playtron fork)

License:        GPL-3.0-or-later
URL:            https://github.com/pop-os/xdg-desktop-portal-cosmic
Source0:        %{name}-%{_arch}.tar.gz

# No BuildRequires - binary is pre-built

# Disable automatic dependency detection for Rust binaries
AutoReqProv:    no

# The portal backend is only reachable through the xdg-desktop-portal frontend,
# which reads the .portal file this package installs.
Requires:       xdg-desktop-portal

# Override the upstream xdg-desktop-portal-cosmic. Fedora ships a higher
# upstream version than this fork carries, so the epoch is what makes ours win.
Provides:       xdg-desktop-portal-cosmic = %{epoch}:%{version}-%{release}
Obsoletes:      xdg-desktop-portal-cosmic < %{epoch}:%{version}

%description
XDG Desktop Portal backend for the COSMIC desktop environment. Provides the
Access, FileChooser, Screenshot, Settings and ScreenCast portal interfaces,
including the interactive screenshot and screencast pickers.

%prep
%autosetup -n %{name} -p1

%build

%install
install -Dm0755 "usr/libexec/%{name}" "%{buildroot}%{_libexecdir}/%{name}"

# D-Bus activation and the systemd user unit
install -Dm0644 "usr/share/dbus-1/services/%{appid}.service" "%{buildroot}%{_datadir}/dbus-1/services/%{appid}.service"
install -Dm0644 "usr/lib/systemd/user/%{appid}.service" "%{buildroot}%{_userunitdir}/%{appid}.service"

# Portal registration + the COSMIC portal preference config
install -Dm0644 "usr/share/xdg-desktop-portal/portals/cosmic.portal" "%{buildroot}%{_datadir}/xdg-desktop-portal/portals/cosmic.portal"
install -Dm0644 "usr/share/xdg-desktop-portal/cosmic-portals.conf" "%{buildroot}%{_datadir}/xdg-desktop-portal/cosmic-portals.conf"

install -Dm0644 "usr/share/licenses/%{name}/LICENSE" "%{buildroot}%{_datadir}/licenses/%{name}/LICENSE"

# Screenshot tool-panel icons
for icon in usr/share/icons/hicolor/scalable/actions/*.svg; do
    install -Dm0644 "${icon}" "%{buildroot}%{_datadir}/icons/hicolor/scalable/actions/$(basename "${icon}")"
done

%files
%license %{_datadir}/licenses/%{name}/LICENSE
%{_libexecdir}/%{name}
%{_datadir}/dbus-1/services/%{appid}.service
%{_userunitdir}/%{appid}.service
%{_datadir}/xdg-desktop-portal/portals/cosmic.portal
%{_datadir}/xdg-desktop-portal/cosmic-portals.conf
%{_datadir}/icons/hicolor/scalable/actions/screenshot-*-symbolic.svg

%changelog
* Wed Jul 29 2026 Playtron <dev@playtron.one> - 0.1.0-1
- Initial RPM package for Playtron fork
