from checks.attestation_guard.near_miss_annotation import near_miss_annotation


def describe_near_miss_annotation():
    def quotes_the_line_and_gives_the_exact_form():
        line = near_miss_annotation("skip-receipt-deletion: y")
        assert line == (
            "::error::'skip-receipt-deletion: y' is not the bypass line. "
            "The exact form is 'allow-receipt-deletion: <reason>', reason required."
        )
