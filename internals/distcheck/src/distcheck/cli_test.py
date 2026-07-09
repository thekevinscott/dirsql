from distcheck.cli import main


def test_group_registers_python_and_node_flows():
    assert set(main.commands) == {"python", "node"}


def test_python_and_node_are_distinct_commands():
    assert main.commands["python"] is not main.commands["node"]
