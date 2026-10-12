"""Entry point: `python -m rmathpy [mcp]` starts the notebook or the MCP server.

With no argument (or only notebook flags such as ``--port``) the web notebook
starts, exactly as before. ``python -m rmathpy mcp [--http]`` starts the MCP
server instead.
"""

import sys


def main(argv=None):
    argv = list(sys.argv[1:] if argv is None else argv)
    if argv and argv[0] == "mcp":
        from .mcp import main as mcp_main
        return mcp_main(argv[1:])
    from .notebook import main as notebook_main
    return notebook_main()


if __name__ == "__main__":
    sys.exit(main())
