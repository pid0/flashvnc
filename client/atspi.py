#!/usr/bin/env python3

import sys, time, subprocess

import gi
gi.require_version("Atspi", "2.0") 
gi.require_version("Gdk", "3.0")
from gi.repository import Gdk
from gi.repository import Atspi

TIMEOUT_IN_S = 4

def query_return_code(boolean):
  if boolean is True:
    return 3
  else:
    return 4

def eprint(*args):
  print(*args, file=sys.stderr)

def move_mouse(relative, x, y):
  action = "mousemove_relative" if relative else "mousemove"
  return subprocess.run(["xdotool", 
    action, "--", str(x), str(y)]).returncode == 0

def generate_mouse_event(kind):
  #return Atspi.generate_mouse_event(x, y, kind)
  action = {
      "c": "click",
      "p": "mousedown",
      "r": "mouseup"
  }[kind[2]]
  button = kind[1]

  return subprocess.run(["xdotool", action, button]).returncode == 0

def generate_keyboard_event(action, key):
  xdotool_action = {
      "key": "key",
      "key-down": "keydown",
      "key-up": "keyup"}[action]
  return subprocess.run(["xdotool", xdotool_action, hex(key)]).returncode == 0

def visit(obj, func):
  func(obj)
  child_count = obj.get_child_count()
  for i in range(child_count):
    visit(obj.get_child_at_index(i), func)

def find_program(pid):
  desktop = Atspi.get_desktop(0)

  child_count = desktop.get_child_count()
  for i in range(child_count):
    child = desktop.get_child_at_index(i)
    if child.get_process_id() == pid:
      return child
#  print("name", child.get_name())
#  print("id", child.get_id())
#  print("pid", child.get_process_id())
#  print("role", child.get_role_name())
#  print()

def find_drawing_area(client):
  ret = [None]

  def test_child(widget):
#    print(widget.get_name())
#    print(widget.get_role_name())
#    try: 
#      n = widget.get_n_actions()
#      for i in range(n):
#        print("action", widget.get_action_name(i))
#    except:
#      pass
    
    if widget.get_role() == Atspi.Role.DRAWING_AREA:
      ret[0] = widget

  visit(client, test_child)
  return ret[0]

def find(pid):
  client = find_program(pid)
  if client is None:
    return None, None
  return client, find_drawing_area(client)

def main(argv):
  if Atspi.init() != 0:
    eprint("could not init")
    return 1

  pid = int(argv[1])
  command = argv[2]
  command_args = argv[3:]

  start_time = time.perf_counter()
  (client, drawing_area) = find(pid)
  while (client is None or drawing_area is None) and command == "wait":
    time.sleep(0.1)
    if time.perf_counter() - start_time > TIMEOUT_IN_S:
      return 1
    (client, drawing_area) = find(pid)

  if client is None:
    eprint("no such pid")
    return 1
  if drawing_area is None:
    eprint("client has no drawing area")
    return 1

  if command == "wait":
    return 0

#    extents = obj.get_extents(Atspi.CoordType.SCREEN)
#    print(extents.x, extents.y, extents.width, extents.height)
#    testPy.grab_focus()
#    print(Atspi.generate_mouse_event(extents.x, extents.y, "b1c")) #button 1 click

  screen_extents = drawing_area.get_extents(Atspi.CoordType.SCREEN)
  window_extents = drawing_area.get_extents(Atspi.CoordType.WINDOW)

  #needs can_focus on target widget
  drawing_area.grab_focus()

  if command == "mouse":
    if command_args[0] == "m":
      relative = command_args[1] == "rel"
      x = int(command_args[2])
      y = int(command_args[3])
      if not relative:
        x += screen_extents.x
        y += screen_extents.y
      return move_mouse(relative, x, y)
    else:
      return generate_mouse_event(command_args[0])
  elif command == "query-screen-size":
    eprint(window_extents.width, window_extents.height)
    return query_return_code(
      window_extents.width == int(command_args[0]) and
      window_extents.height == int(command_args[1]))
  elif command == "take-screenshot":
    dest = command_args[0]
    screen = Gdk.get_default_root_window()
    pixbuf = Gdk.pixbuf_get_from_window(screen, 
        screen_extents.x, screen_extents.y,
        screen_extents.width, screen_extents.height)
    pixbuf.savev(dest, "png", [], [])
  elif command == "focus":
    time.sleep(0.5)
    drawing_area.grab_focus()
    time.sleep(0.5)
  elif command.startswith("key"):
    return generate_keyboard_event(command, int(command_args[0]))
  elif command == "resize":
    width, height = (s for s in command_args[:2])
    if subprocess.run(["xdotool", "getactivewindow",
      "windowsize", "--sync", width, height]).returncode != 0:
      return False
  else:
    eprint("no such command")
    return 2

  return 0

return_code = main(sys.argv)
if return_code is True:
  sys.exit(0)
elif return_code is False:
  sys.exit(1)
else:
  sys.exit(return_code)
