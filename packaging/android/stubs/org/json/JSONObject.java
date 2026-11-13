package org.json;

import java.util.Iterator;

public class JSONObject {
    public static final Object NULL = new Object();
    public JSONObject() {}
    public JSONObject(String json) throws JSONException {}
    public JSONObject put(String name, Object value) throws JSONException { return this; }
    public JSONObject put(String name, boolean value) throws JSONException { return this; }
    public JSONObject put(String name, int value) throws JSONException { return this; }
    public JSONObject put(String name, long value) throws JSONException { return this; }
    public boolean has(String name) { return false; }
    public boolean isNull(String name) { return true; }
    public String optString(String name) { return ""; }
    public String optString(String name, String fallback) { return fallback; }
    public boolean optBoolean(String name) { return false; }
    public boolean optBoolean(String name, boolean fallback) { return fallback; }
    public int optInt(String name) { return 0; }
    public int optInt(String name, int fallback) { return fallback; }
    public long optLong(String name) { return 0L; }
    public long optLong(String name, long fallback) { return fallback; }
    public JSONObject optJSONObject(String name) { return null; }
    public JSONArray optJSONArray(String name) { return null; }
    public Iterator<String> keys() { return null; }
    public Object remove(String name) { return null; }
    public static String quote(String data) { return "\"\""; }
    @Override
    public String toString() { return "{}"; }
}
