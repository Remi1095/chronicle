
set ADDR 'http://localhost:3000/api'

curl -X POST $ADDR/dashboards \
    -H "Content-Type: application/json" \
    -d '{"name": "Test dashboard", "description": "Some description"}'


set json '{
    "table_id": 1,
    "name": "Test chart",
    "chart_kind": "Bar"
}'

curl -X POST $ADDR/dashboards/1/charts \
    -H "Content-Type: application/json" \
    -d "$json"

set json '[
     {
        "field_id": 1,
        "axis_kind": "X",
        "aggregate": null
    },
    {
        "field_id": 2,
        "axis_kind": "Y",
        "aggregate": "Average"
    },
    {
        "field_id": 2,
        "axis_kind": "Label",
        "aggregate": "Sum"
    }
]'


curl -X PUT $ADDR/dashboards/1/charts/2/axes \
    -H "Content-Type: application/json" \
    -d "$json"


function test-chart-data
    curl -X GET $ADDR/dashboards/1/charts/1/data &

    curl -X GET $ADDR/dashboards/1/charts/2/data
end
