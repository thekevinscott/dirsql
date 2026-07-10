
{% for section_name, section in sections.items() %}{% for category, val in definitions.items() if category in section %}### {{ val.name }}

{% for text in section[category] %}{{ text }}

{% endfor %}{% endfor %}{% endfor %}
