# Herdr Shell Pool

Vocabulary for keeping shells ready for new Herdr tabs, splits, and workspaces.

## Language

**Warm shell**:
A shell that is already running and waiting to be used in a new tab, split, or workspace. It is distinct from the pane that contains it.

**Ready warm shell**:
A warm shell whose startup setup has finished and which is idle at a prompt.

**Shell pool**:
The collection of warm shells held ready for new tabs, splits, and workspaces.

**Pool workspace**:
The Herdr workspace that contains the panes holding the shell pool.

**Moving a warm shell**:
Moving the pane containing a warm shell from the pool workspace into the requested tab, split, or workspace, while the shell keeps running.
_Avoid_: Taking a warm shell

**Cold shell**:
A newly started shell, rather than an already-running shell moved from the shell pool.

**Warm-shell failure**:
A failure to provide a warm shell for a requested new tab, split, or workspace. Opening the requested destination with a cold shell is recovery from this failure, not normal operation.
