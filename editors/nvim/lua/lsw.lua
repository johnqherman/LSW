local M = {}

M.config = { path = "lsw" }

local function run(args)
  local cmd = M.config.path .. " " .. table.concat(args, " ")
  vim.cmd("botright split | terminal " .. cmd)
end

function M.env()
  local handle = io.popen(M.config.path .. " --format json ide env")
  if not handle then
    return nil
  end
  local out = handle:read("*a")
  handle:close()
  local ok, decoded = pcall(vim.json.decode, out)
  if not ok then
    return nil
  end
  return decoded
end

function M.build()
  run({ "build" })
end

function M.test()
  run({ "test" })
end

function M.verify()
  run({ "verify", "--native-windows" })
end

function M.run(program)
  program = program or vim.fn.input("Program to run: ", "build/app.exe")
  if program ~= "" then
    run({ "run", program })
  end
end

function M.configure()
  local env = M.env()
  if not env then
    vim.notify("lsw ide env failed", vim.log.levels.ERROR)
    return
  end
  vim.g.lsw_compiler = env.compiler
  vim.g.lsw_include_paths = env.includePaths
  vim.notify("LSW: configured for " .. tostring(env.target))
end

function M.setup_dap()
  local ok, dap = pcall(require, "dap")
  if not ok then
    return
  end
  dap.adapters.lsw = { type = "executable", command = M.config.path, args = { "dap" } }
  dap.configurations.c = {
    {
      type = "lsw",
      request = "launch",
      name = "LSW: debug PE",
      program = function()
        return vim.fn.input("PE path: ", vim.fn.getcwd() .. "/build/", "file")
      end,
    },
  }
  dap.configurations.cpp = dap.configurations.c
  dap.configurations.rust = dap.configurations.c
end

function M.setup(opts)
  M.config = vim.tbl_extend("force", M.config, opts or {})
  M.setup_dap()
  vim.api.nvim_create_user_command("LswBuild", M.build, {})
  vim.api.nvim_create_user_command("LswTest", M.test, {})
  vim.api.nvim_create_user_command("LswVerify", M.verify, {})
  vim.api.nvim_create_user_command("LswRun", function(a)
    M.run(a.args ~= "" and a.args or nil)
  end, { nargs = "?" })
  vim.api.nvim_create_user_command("LswConfigure", M.configure, {})
  vim.api.nvim_create_user_command("LswEnv", function()
    local env = M.env()
    if env then
      vim.notify(vim.inspect(env))
    end
  end, {})
end

return M
