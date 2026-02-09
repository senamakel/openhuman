const Conversations = () => {
  return (
    <div className="min-h-full relative">
      <div className="relative z-10 min-h-full flex flex-col">
        <div className="flex-1 flex items-center justify-center p-4">
          <div className="max-w-md w-full">
            <div className="glass rounded-3xl p-8 shadow-large animate-fade-up text-center">
              <div className="flex justify-center mb-4">
                <svg
                  className="w-12 h-12 text-primary-400 opacity-60"
                  fill="none"
                  stroke="currentColor"
                  viewBox="0 0 24 24">
                  <path
                    strokeLinecap="round"
                    strokeLinejoin="round"
                    strokeWidth={1.5}
                    d="M8 12h.01M12 12h.01M16 12h.01M21 12c0 4.418-4.03 8-9 8a9.863 9.863 0 01-4.255-.949L3 20l1.395-3.72C3.512 15.042 3 13.574 3 12c0-4.418 4.03-8 9-8s9 3.582 9 8z"
                  />
                </svg>
              </div>
              <h1 className="text-xl font-bold mb-2">Conversations</h1>
              <p className="text-sm opacity-60">Your conversations will appear here</p>
            </div>
          </div>
        </div>
      </div>
    </div>
  );
};

export default Conversations;
