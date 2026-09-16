"""Colocated unit tests for the host probe behind the memory cap."""

from unittest import mock

from checks.preflight.host import Host, detect_host

MEMINFO = "MemTotal:       48369800 kB\n"


def probe(which="/usr/bin/systemd-run", opened=None, parse=None):
    with (
        mock.patch("checks.preflight.host.shutil.which", return_value=which) as which_mock,
        mock.patch("checks.preflight.host.open", opened or mock.mock_open(read_data=MEMINFO)) as open_mock,
        mock.patch("checks.preflight.host.mem_total_kb", parse or mock.Mock(return_value=48369800)) as parse_mock,
    ):
        host = detect_host()
    return host, which_mock, open_mock, parse_mock


def describe_detect_host():
    def it_reports_systemd_run_on_path_and_the_parsed_mem_total():
        host, which, opened, parsed = probe()
        assert host == Host(systemd_run=True, mem_total_kb=48369800)
        which.assert_called_once_with("systemd-run")
        opened.assert_called_once_with("/proc/meminfo", encoding="utf-8")
        parsed.assert_called_once_with(MEMINFO)

    def it_reports_systemd_run_absent_when_the_path_lookup_finds_nothing():
        host, _which, _opened, _parsed = probe(which=None)
        assert host.systemd_run is False

    def it_reports_mem_total_unknown_when_meminfo_cannot_be_read():
        host, _which, _opened, parsed = probe(opened=mock.Mock(side_effect=OSError))
        assert host.mem_total_kb is None
        assert not parsed.called

    def it_reports_mem_total_unknown_when_meminfo_names_no_mem_total():
        host, _which, _opened, _parsed = probe(parse=mock.Mock(side_effect=ValueError))
        assert host.mem_total_kb is None
