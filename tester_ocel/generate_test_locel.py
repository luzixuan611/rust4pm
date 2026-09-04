import r4pm
import r4pm.bindings as b

# 1. create SlimLinkedOCEL
locel = b.locel_new()

# 2. register types
b.locel_add_event_type(locel, "place_order", [])
b.locel_add_event_type(locel, "order_confirm", [])

b.locel_add_object_type(locel, "order")
b.locel_add_object_type(locel, "item", [{"name": "price", "type": "float"}])

# 3. create objects
order = b.locel_add_object(locel, "order")

items = []
for j in range(4):
    price_history = [[("1970-01-01T00:00:00Z", round((j + 1) * 10.0, 2))]]
    item = b.locel_add_object(locel, "item", None, price_history)
    items.append(item)

# 4. create events and binding relationships

# e1: place_order (10:00) binds order with 4 items
e1 = b.locel_add_event(locel, "place_order", "2026-03-02T10:00:00Z", None, [])
b.locel_add_e2o(locel, e1, order, "of")
for item in items:
    b.locel_add_e2o(locel, e1, item, "with")

# e2: order_confirm (11:00, takes 1h <= 2.5h) binds first 3 items
e2 = b.locel_add_event(locel, "order_confirm", "2026-03-02T11:00:00Z", None, [])
b.locel_add_e2o(locel, e2, order, "of")
for item in items[:3]:
    b.locel_add_e2o(locel, e2, item, "with")

# e3: order_confirm (14:00, takes 4h > 2.5h) binds the 4th item
e3 = b.locel_add_event(locel, "order_confirm", "2026-03-02T14:00:00Z", None, [])
b.locel_add_e2o(locel, e3, order, "of")
b.locel_add_e2o(locel, e3, items[3], "with")

# 5. export locel to JSON
output_path = "./data/ocel2-p2p.json"
r4pm.export_item(locel, output_path)
print(f"Exported test dataset to: {output_path}")