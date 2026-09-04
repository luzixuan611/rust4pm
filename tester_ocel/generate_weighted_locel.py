import r4pm
import r4pm.bindings as b

locel = b.locel_new()

# 注册事件类型与对象类型（声明 price 属性）
b.locel_add_event_type(locel, "place_order", [])
b.locel_add_event_type(locel, "order_confirm", [])

b.locel_add_object_type(locel, "order")
b.locel_add_object_type(locel, "item", [{"name": "price", "type": "float"}])

# 实例化对象并写入价格属性
order = b.locel_add_object(locel, "order")

# 商品定义: [(名称, 价格)]
products = [
    ("monitor", 300.0),
    ("keyboard", 100.0),
    ("mouse_1", 30.0),
    ("mouse_2", 30.0),
]

items = []
for name, price in products:
    price_history = [[("1970-01-01T00:00:00Z", price)]]
    item_ref = b.locel_add_object(locel, "item", name, price_history)
    items.append(item_ref)

# e1: place_order (10:00) 关联全部 4 件物品
e1 = b.locel_add_event(locel, "place_order", "2026-03-02T10:00:00Z", None, [])
b.locel_add_e2o(locel, e1, order, "of")
for item in items:
    b.locel_add_e2o(locel, e1, item, "with")

# e2: order_confirm (11:00, 耗时 1h <= 2.5h) 确认前 3 件 (显示器、键盘、鼠标1)
e2 = b.locel_add_event(locel, "order_confirm", "2026-03-02T11:00:00Z", None, [])
b.locel_add_e2o(locel, e2, order, "of")
for item in items[:3]:
    b.locel_add_e2o(locel, e2, item, "with")

# e3: order_confirm (14:00, 耗时 4h > 2.5h) 确认最后 1 件 (鼠标2)
e3 = b.locel_add_event(locel, "order_confirm", "2026-03-02T14:00:00Z", None, [])
b.locel_add_e2o(locel, e3, order, "of")
b.locel_add_e2o(locel, e3, items[3], "with")

# 输出到 p2p 路径供 Rust 评测
output_path = "./data/ocel2_weighted.json"
r4pm.export_item(locel, output_path)
print(f"Exported weighted test dataset to: {output_path}")