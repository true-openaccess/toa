-- pandoc filter for the TOA standard: section anchors and links for section references.
-- Used for the web page and the PDF alike (scripts/spec/build.py).
--
-- * A heading "3.2.1 Common fields" gets the id "3.2.1"; "Annex A …" / "附录 A…" gets "annex-a".
--   Headings that already have an id (the type specifications) keep it.
-- * The level-1 heading (the title) is dropped: the page and the PDF carry the title themselves.
-- * In the text, a section number such as 3.2 or 3.2.1 becomes a link to its section.
--   Numbers in code are not touched; write version numbers as code.

local html = FORMAT:match("html") ~= nil

local function key_of(text)
  local num = text:match("^(%d+[%.%d]*)%s")
  if num then return num:gsub("%.$", "") end
  local letter = text:match("^Annex (%u)") or text:match("^附录 (%u)")
  if letter then return "annex-" .. letter:lower() end
  return nil
end

-- the inlines of a heading without its leading label ("3.2", "Annex A", "附录 A") and the space after it
local function drop_prefix(inlines, label)
  local left = label
  local out = {}
  local done = false
  for _, inline in ipairs(inlines) do
    if done then
      table.insert(out, inline)
    elseif inline.t == "Space" then
      if left == "" then done = true else left = left:gsub("^ ", "") end
    elseif inline.t == "Str" then
      local s = inline.text
      if s:sub(1, #left) == left then
        local remainder = s:sub(#left + 1)
        left = ""
        if remainder ~= "" then table.insert(out, pandoc.Str(remainder)); done = true end
      elseif left:sub(1, #s) == s then
        left = left:sub(#s + 1)
      else
        done = true; table.insert(out, inline)
      end
    else
      done = true; table.insert(out, inline)
    end
  end
  return out
end

function Header(el)
  if el.level == 1 then return {} end
  local text = pandoc.utils.stringify(el.content)
  local key = key_of(text)
  if el.identifier == "" and key then el.identifier = key end
  if html and key and not el.classes:includes("type-spec") then
    local label = text:match("^(%S+)")
    if key:match("^annex") then label = text:match("^(Annex %u)") or text:match("^(附录 %u)") end
    local rest = drop_prefix(el.content, label)
    local content = {pandoc.Span(pandoc.Str(label), {class = "sec"})}
    for _, inline in ipairs(rest) do table.insert(content, inline) end
    table.insert(content, pandoc.Link(pandoc.Str("#"), "#" .. el.identifier, "",
      {class = "anchor", ["aria-label"] = "Link to " .. el.identifier}))
    el.content = content
  end
  return el
end

-- section numbers in plain text: 2.1, 3.2.1 (not 1.62: at most two digits after a dot)
local PATTERN = "%f[%d]%d%d?%.%d%d?[%.%d]*"

local function split(str)
  local out = {}
  local pos = 1
  while true do
    local s, e = str:find(PATTERN, pos)
    if not s then break end
    local num = str:sub(s, e)
    -- trim a trailing dot ("see 3.2." ends a sentence)
    while num:sub(-1) == "." do num = num:sub(1, -2); e = e - 1 end
    local before, after = str:sub(s - 1, s - 1), str:sub(e + 1, e + 1)
    local inside_word = before:match("[%w/_-]") or after:match("[%w/_-]") or (after == "." and str:sub(e + 2, e + 2):match("%d"))
    if inside_word then
      pos = e + 1
    else
      if s > pos then table.insert(out, pandoc.Str(str:sub(pos, s - 1))) end
      table.insert(out, pandoc.Link(pandoc.Str(num), "#" .. num, "", {class = "ref"}))
      pos = e + 1
    end
  end
  if #out == 0 then return nil end
  if pos <= #str then table.insert(out, pandoc.Str(str:sub(pos))) end
  return out
end

local function link_refs(inlines)
  local out = pandoc.Inlines({})
  local changed = false
  for _, inline in ipairs(inlines) do
    if inline.t == "Str" then
      local parts = split(inline.text)
      if parts then
        changed = true
        for _, p in ipairs(parts) do out:insert(p) end
      else
        out:insert(inline)
      end
    else
      out:insert(inline)
    end
  end
  if changed then return out end
  return nil
end

return {
  {Header = Header},
  {
    Para = function(el) local r = link_refs(el.content); if r then el.content = r; return el end end,
    Plain = function(el) local r = link_refs(el.content); if r then el.content = r; return el end end,
  },
}
