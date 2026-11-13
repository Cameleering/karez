-- Deterministic seed: every value is a pure function of the row number. No random().
SET ROLE karez_scram;

INSERT INTO categories (id, name, parent_id)
SELECT i, 'Category ' || i, CASE WHEN i > 10 THEN (i % 10) + 1 END
  FROM generate_series(1, 50) AS i;

INSERT INTO customers (id, email, name, phone, notes, created_at, is_active, loyalty_points, status)
SELECT i,
       'user' || i || '@example.test',
       'Customer ' || i || ' ' || md5(i::text),
       CASE WHEN i % 3 = 0 THEN NULL ELSE '+1-555-' || lpad(((i * 7919) % 10000000)::text, 7, '0') END,
       CASE WHEN i % 10 = 0 THEN repeat('note ', (i % 50) + 1) END,
       timestamptz '2020-01-01 00:00:00+00' + ((i * 4001) % 150000000) * interval '1 second',
       i % 17 <> 0,
       i % 1000,
       (ARRAY['new','active','vip','closed'])[(i % 4) + 1]
  FROM generate_series(1, 20000) AS i;

INSERT INTO addresses (id, customer_id, line1, line2, city, postal_code, country, is_default)
SELECT i,
       (i % 20000) + 1,
       i || ' Main Street',
       CASE WHEN i % 5 = 0 THEN 'Apt ' || (i % 100) END,
       (ARRAY['Springfield','Bengaluru','Berlin','Osaka','São Paulo'])[(i % 5) + 1],
       lpad(((i * 31) % 100000)::text, 5, '0'),
       (ARRAY['US','IN','DE','JP','BR'])[(i % 5) + 1],
       i <= 20000
  FROM generate_series(1, 30000) AS i;

INSERT INTO products (id, sku, name, description, category_id, price_cents, weight_kg, rating,
                      created_at, discontinued_on)
SELECT i,
       'SKU-' || lpad(i::text, 6, '0'),
       'Product ' || i,
       CASE WHEN i % 7 = 0 THEN 'Descripción ünïcode 日本語 ' || i
            WHEN i % 11 = 0 THEN NULL
            ELSE 'Plain description ' || i END,
       (i % 50) + 1,
       ((i * 37) % 100000) + 99,
       CASE WHEN i % 4 = 0 THEN NULL ELSE ((i % 500) / 8.0)::float4 END,
       ((i % 50) / 10.0)::float8,
       timestamptz '2021-01-01 00:00:00+00' + (i * 3600) * interval '1 second',
       CASE WHEN i % 20 = 0 THEN date '2024-01-01' + (i % 300) END
  FROM generate_series(1, 5000) AS i;

INSERT INTO inventory (product_id, on_hand, reserved)
SELECT i, 100 + (i % 900), 0 FROM generate_series(1, 5000) AS i;

INSERT INTO orders (id, customer_id, ship_address_id, placed_at, status, note)
SELECT i,
       c,
       CASE WHEN c = 1 THEN 20000 ELSE c - 1 END,
       timestamptz '2023-01-01 00:00:00+00' + (i * 211) * interval '1 second',
       (ARRAY['placed','paid','shipped','cancelled'])[(i % 4) + 1],
       CASE WHEN i % 25 = 0 THEN 'Leave at the door, "gate code" 1234' || E'\n' || 'Call on arrival' END
  FROM (SELECT i, ((i * 31) % 20000) + 1 AS c FROM generate_series(1, 150000) AS i) s;

INSERT INTO order_items (id, order_id, line_no, product_id, qty, unit_price_cents, discount_pct)
SELECT i,
       (i % 150000) + 1,
       (i / 150000) + 1,
       p,
       ((i % 5) + 1)::int2,
       ((p * 37) % 100000) + 99,
       CASE WHEN i % 3 = 0 THEN NULL ELSE ((i % 4) * 0.05)::float4 END
  FROM (SELECT i, ((i * 13) % 5000) + 1 AS p FROM generate_series(1, 400000) AS i) s;

UPDATE orders o
   SET total_cents = t.total
  FROM (SELECT order_id, sum(qty * unit_price_cents) AS total FROM order_items GROUP BY order_id) t
 WHERE t.order_id = o.id;

INSERT INTO payments (id, order_id, amount_cents, method, paid_at, reference)
SELECT o.id, o.id, o.total_cents,
       (ARRAY['card','transfer','cash'])[(o.id % 3) + 1],
       o.placed_at + interval '1 hour',
       md5('payment' || o.id)::uuid
  FROM orders o WHERE o.id <= 120000;

INSERT INTO refunds (id, payment_id, amount_cents, reason, refunded_at)
SELECT i, i * 13, least(1000, p.amount_cents + 1),
       CASE WHEN i % 3 = 0 THEN NULL ELSE 'Customer returned item ' || i END,
       p.paid_at + interval '3 days'
  FROM generate_series(1, 8000) AS i
  JOIN payments p ON p.id = i * 13;

INSERT INTO invoices (id, order_id, issued_on, due_on, pdf)
SELECT o.id, o.id, (o.placed_at)::date, (o.placed_at)::date + 30,
       CASE WHEN o.id % 1000 = 0 THEN decode(repeat('25504446', ((o.id % 50) + 1)::int), 'hex') END
  FROM orders o WHERE o.id <= 100000;

INSERT INTO inventory_movements (id, product_id, delta, reason, moved_at, order_id)
SELECT i, ((i * 7) % 5000) + 1, (i % 21) - 10,
       (ARRAY['sale','restock','adjustment'])[(i % 3) + 1],
       timestamptz '2023-01-01 00:00:00+00' + (i * 97) * interval '1 second',
       CASE WHEN i % 2 = 0 THEN NULL ELSE (i % 150000) + 1 END
  FROM generate_series(1, 300000) AS i;

INSERT INTO audit_log (id, at, actor, action, details)
SELECT i,
       timestamptz '2023-01-01 00:00:00+00' + (i * 53) * interval '1 second',
       'user' || (i % 500),
       (ARRAY['login','update','delete','export'])[(i % 4) + 1],
       CASE WHEN i % 1000 = 0 THEN 'it''s "quoted" \ back' || E'\n' || 'new line 🎉 日本語'
            WHEN i % 7 = 0 THEN NULL
            ELSE 'event ' || i || ' ' || md5(i::text) END
  FROM generate_series(1, 400000) AS i;

-- Move every identity sequence past the explicit ids used above.
SELECT setval(pg_get_serial_sequence('customers', 'id'), 20000);
SELECT setval(pg_get_serial_sequence('addresses', 'id'), 30000);
SELECT setval(pg_get_serial_sequence('products', 'id'), 5000);
SELECT setval(pg_get_serial_sequence('orders', 'id'), 150000);
SELECT setval(pg_get_serial_sequence('order_items', 'id'), 400000);
SELECT setval(pg_get_serial_sequence('payments', 'id'), 120000);
SELECT setval(pg_get_serial_sequence('refunds', 'id'), 8000);
SELECT setval(pg_get_serial_sequence('invoices', 'id'), 100000);
SELECT setval(pg_get_serial_sequence('inventory_movements', 'id'), 300000);
SELECT setval(pg_get_serial_sequence('audit_log', 'id'), 400000);
SELECT setval('order_no_seq', 1000000);

RESET ROLE;
