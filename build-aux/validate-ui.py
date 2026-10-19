#!/usr/bin/env python3
"""
Check the GtkBuilder files against the GTK and libadwaita of this system.

Every object of a GTK, libadwaita or libshumate class must exist, and every
property, binding and signal set on it must exist on that class. Objects of
the application's own classes (Papl*) are not checked here: their templates
are built by the `templates_build_without_warnings` test.

gtk4-builder-tool validate cannot do this: it stops at the first object of a
class it does not know (any Adw* or Papl* one).

Usage: validate-ui.py PATH...  (.ui files or directories searched for them)
"""

import pathlib
import sys
import warnings
import xml.etree.ElementTree as ET

import gi

gi.require_version("Gtk", "4.0")
gi.require_version("Adw", "1")
from gi.repository import Adw, Gdk, Gio, GObject, Gtk  # noqa: E402

# Classes of these libraries are checked; any other class (Papl*, Content,
# Ori* from libadwaita-rs' companion crate origami...) belongs to the
# application or a Rust crate and is built by the template test instead.
NAMESPACES = {"Gtk": Gtk, "Gdk": Gdk, "Adw": Adw, "G": Gio}
try:
    gi.require_version("Shumate", "1.0")
    from gi.repository import Shumate  # noqa: E402

    NAMESPACES["Shumate"] = Shumate
except (ImportError, ValueError):
    print("libshumate typelib not found: Shumate* objects are not checked", file=sys.stderr)
    NAMESPACES["Shumate"] = None


def namespace(name):
    """Return (module, short name) of a library class, None for others."""
    for prefix, module in sorted(NAMESPACES.items(), key=lambda i: -len(i[0])):
        rest = name[len(prefix):]
        if name.startswith(prefix) and rest[:1].isupper():
            return module, rest
    return None


def is_custom(name):
    found = namespace(name)
    return found is None or found[0] is None


def lookup_class(name):
    """Return the Python class for a GType name of a checked library, or None."""
    found = namespace(name)
    if found is None or found[0] is None:
        return None
    cls = getattr(found[0], found[1], None)
    if isinstance(cls, type) and issubclass(cls, GObject.Object):
        with warnings.catch_warnings():
            warnings.simplefilter("ignore", DeprecationWarning)
            GObject.type_class_ref(cls.__gtype__)  # installs its signals
        return cls
    return None


def has_property(cls, name):
    return cls.find_property(name.replace("_", "-")) is not None


def has_signal(cls, name):
    name = name.split("::", 1)[0]
    return GObject.signal_lookup(name.replace("_", "-"), cls.__gtype__) != 0


def check_tree(root, where, errors):
    for element in root.iter():
        if element.tag == "object":
            check_object(element, element.get("class"), where, errors)
        elif element.tag == "template":
            parent = element.get("parent")
            if parent is None:
                # the template of a list item factory: <template class="GtkListItem">
                check_object(element, element.get("class"), where, errors)
            elif not is_custom(parent) and lookup_class(parent) is None:
                errors.append(f"{where}: template {element.get('class')}: unknown parent class {parent}")

        # list item factories keep their own interface in the "bytes" property
        if element.tag == "property" and element.get("name") == "bytes" and element.text:
            text = element.text.strip()
            if text.startswith("<?xml") or text.startswith("<interface"):
                check_tree(ET.fromstring(text.encode()), f"{where} (factory)", errors)


def check_object(element, class_name, where, errors):
    if is_custom(class_name):
        return

    cls = lookup_class(class_name)
    if cls is None:
        errors.append(f"{where}: unknown object type {class_name}")
        return

    for child in element:
        name = child.get("name")
        if child.tag in ("property", "binding") and not has_property(cls, name):
            errors.append(f"{where}: invalid property {class_name}.{name}")
        elif child.tag == "signal" and not has_signal(cls, name):
            errors.append(f"{where}: invalid signal {class_name}::{name}")


def ui_files(paths):
    for path in map(pathlib.Path, paths):
        if path.is_dir():
            yield from sorted(path.rglob("*.ui"))
        else:
            yield path


def main(args):
    if not args:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    errors = []
    count = 0
    for path in ui_files(args):
        count += 1
        try:
            check_tree(ET.parse(path).getroot(), str(path), errors)
        except ET.ParseError as e:
            errors.append(f"{path}: {e}")

    for error in errors:
        print(error, file=sys.stderr)
    print(f"{count} files checked, {len(errors)} errors (GTK {Gtk.get_major_version()}."
          f"{Gtk.get_minor_version()}.{Gtk.get_micro_version()}, libadwaita "
          f"{Adw.get_major_version()}.{Adw.get_minor_version()}.{Adw.get_micro_version()})")
    return 1 if errors or not count else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
