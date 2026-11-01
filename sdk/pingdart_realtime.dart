// PingDart Realtime Client SDK for Flutter & Dart
// Firebase-like Real-Time Subscriptions and Storage Client

import 'dart:async';
import 'dart:convert';
import 'package:http/http.dart' as http;
import 'package:web_socket_channel/web_socket_channel.dart';

class PingDartClient {
  final String httpUrl;
  late final String wsUrl;
  final String? token;
  final String database;
  WebSocketChannel? _channel;
  final Map<String, StreamController<Map<String, dynamic>>> _tableStreams = {};

  PingDartClient({
    required this.httpUrl,
    this.token,
    this.database = 'default',
  }) {
    final cleanUrl = httpUrl.replaceAll(RegExp(r'/$'), '');
    wsUrl = cleanUrl.replaceFirst(RegExp(r'^http'), 'ws') + '/v1/realtime';
  }

  void connect() {
    if (_channel != null) return;
    final uri = Uri.parse('$wsUrl?token=${token ?? ''}');
    _channel = WebSocketChannel.connect(uri);

    _channel!.stream.listen(
      (data) {
        try {
          final Map<String, dynamic> msg = jsonDecode(data);
          final String? table = msg['table'];
          if (table != null && _tableStreams.containsKey(table)) {
            _tableStreams[table]!.add(msg);
          }
        } catch (_) {}
      },
      onDone: () {
        _channel = null;
        // Auto-reconnect after 3s
        Timer(const Duration(seconds: 3), () => connect());
      },
    );
  }

  PingDartCollection collection(String table) {
    return PingDartCollection(this, table);
  }

  /// List all tables and schemas in the database (GET /v1/schema/tables)
  Future<Map<String, dynamic>> listTables() async {
    final res = await http.get(
      Uri.parse('$httpUrl/v1/schema/tables?database=$database'),
      headers: {
        'Content-Type': 'application/json',
        if (token != null) 'Authorization': 'Bearer $token',
        'X-PingDart-Project': database,
      },
    );
    return jsonDecode(res.body);
  }

  /// Execute SQL or structured query (POST /v1/query)
  Future<Map<String, dynamic>> query(String sql) async {
    final res = await http.post(
      Uri.parse('$httpUrl/v1/query'),
      headers: {
        'Content-Type': 'application/json',
        if (token != null) 'Authorization': 'Bearer $token',
        'X-PingDart-Project': database,
      },
      body: jsonEncode({'query': sql, 'database': database}),
    );
    return jsonDecode(res.body);
  }
}

class PingDartCollection {
  final PingDartClient client;
  final String table;

  PingDartCollection(this.client, this.table);

  Map<String, String> _buildHeaders() {
    return {
      'Content-Type': 'application/json',
      if (client.token != null) 'Authorization': 'Bearer ${client.token}',
      'X-PingDart-Project': client.database,
    };
  }

  /// Real-time stream of all INSERT, UPDATE, DELETE, and SNAPSHOT events
  Stream<Map<String, dynamic>> onSnapshot() {
    if (!client._tableStreams.containsKey(table)) {
      client._tableStreams[table] = StreamController<Map<String, dynamic>>.broadcast();
      client.connect();
      // Send subscribe frame
      final subMsg = jsonEncode({
        'action': 'subscribe',
        'table': table,
        'database': client.database,
        'fetch_snapshot': true,
      });
      client._channel?.sink.add(subMsg);
    }
    return client._tableStreams[table]!.stream;
  }

  /// Query / list records with PRD Section 6.4 rich filtering, ordering, select projection, and pagination
  Future<Map<String, dynamic>> list({
    Map<String, String>? filters,
    String? select,
    String? order,
    int? limit,
    int? offset,
    int? page,
    String? search,
  }) async {
    final queryParams = <String, String>{
      'database': client.database,
      if (select != null) 'select': select,
      if (order != null) 'order': order,
      if (limit != null) 'limit': limit.toString(),
      if (offset != null) 'offset': offset.toString(),
      if (page != null) 'page': page.toString(),
      if (search != null) 'q': search,
      if (filters != null) ...filters,
    };

    final uri = Uri.parse('${client.httpUrl}/v1/data/$table').replace(queryParameters: queryParams);
    final res = await http.get(uri, headers: _buildHeaders());
    return jsonDecode(res.body);
  }

  /// Get single record by ID (GET /v1/data/:table/:id)
  Future<Map<String, dynamic>> get(String id, {String? select}) async {
    var uri = Uri.parse('${client.httpUrl}/v1/data/$table/$id');
    if (select != null) {
      uri = uri.replace(queryParameters: {'select': select, 'database': client.database});
    }
    final res = await http.get(uri, headers: _buildHeaders());
    return jsonDecode(res.body);
  }

  /// Create single record (POST /v1/data/:table)
  Future<Map<String, dynamic>> add(Map<String, dynamic> data) async {
    final res = await http.post(
      Uri.parse('${client.httpUrl}/v1/data/$table'),
      headers: _buildHeaders(),
      body: jsonEncode(data),
    );
    return jsonDecode(res.body);
  }

  /// Create batch records (POST /v1/data/:table)
  Future<Map<String, dynamic>> addBatch(List<Map<String, dynamic>> records) async {
    final res = await http.post(
      Uri.parse('${client.httpUrl}/v1/data/$table'),
      headers: _buildHeaders(),
      body: jsonEncode(records),
    );
    return jsonDecode(res.body);
  }

  /// Update record (PATCH /v1/data/:table/:id)
  Future<Map<String, dynamic>> update(String id, Map<String, dynamic> data) async {
    final res = await fetchPatch(
      Uri.parse('${client.httpUrl}/v1/data/$table/$id'),
      headers: _buildHeaders(),
      body: jsonEncode(data),
    );
    return jsonDecode(res.body);
  }

  /// Delete record (DELETE /v1/data/:table/:id)
  Future<Map<String, dynamic>> delete(String id) async {
    final res = await http.delete(
      Uri.parse('${client.httpUrl}/v1/data/$table/$id'),
      headers: _buildHeaders(),
    );
    return jsonDecode(res.body);
  }

  /// Inspect table schema (GET /v1/schema/tables/:table)
  Future<Map<String, dynamic>> schema() async {
    final res = await http.get(
      Uri.parse('${client.httpUrl}/v1/schema/tables/$table?database=${client.database}'),
      headers: _buildHeaders(),
    );
    return jsonDecode(res.body);
  }

  Future<http.Response> fetchPatch(Uri url, {Map<String, String>? headers, Object? body}) {
    return http.patch(url, headers: headers, body: body);
  }
}
