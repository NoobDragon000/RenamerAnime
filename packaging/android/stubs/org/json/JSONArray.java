package org.json;

import java.util.Collection;

public class JSONArray {
    public JSONArray() {}
    public JSONArray(String json) throws JSONException {}
    public JSONArray(Collection<?> copyFrom) {}
    public int length() { return 0; }
    public String optString(int index) { return ""; }
    public String optString(int index, String fallback) { return fallback; }
    public int optInt(int index) { return 0; }
    public JSONObject optJSONObject(int index) { return null; }
    public JSONArray put(Object value) { return this; }
    public JSONArray put(int value) { return this; }
    @Override
    public String toString() { return "[]"; }
}
