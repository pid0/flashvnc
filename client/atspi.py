#!/usr/bin/env python3

import sys, time

import gi
gi.require_version("Atspi", "2.0") 
from gi.repository import Atspi

TIMEOUT_IN_S = 4

def query_return_code(boolean):
  if boolean is True:
    return 3
  else:
    return 4

def eprint(*args):
  print(*args, file=sys.stderr)

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

  #TODO does not work
  client.grab_focus()
  drawing_area.grab_focus()

  if command == "mouse":
    return Atspi.generate_mouse_event(
        screen_extents.x + int(command_args[1]),
        screen_extents.y + int(command_args[2]),
        command_args[0])
  elif command == "query-screen-size":
    eprint(window_extents.width, window_extents.height)
    return query_return_code(
      window_extents.width == int(command_args[0]) and
      window_extents.height == int(command_args[1]))
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
